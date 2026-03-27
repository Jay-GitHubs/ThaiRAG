use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use thairag_core::ThaiRagError;
use thairag_core::types::{DocId, Entity, EntityId, KnowledgeGraph, Relation, WorkspaceId};
use tracing::{info, warn};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::knowledge_graph::{extract_entities_from_text, extract_relations_from_text};

// ── Query params ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EntityFilter {
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    pub q: Option<String>,
}

// ── Response types ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct EntityWithRelations {
    #[serde(flatten)]
    pub entity: Entity,
    pub relations: Vec<Relation>,
}

#[derive(Serialize)]
pub struct ExtractionResult {
    pub entities_created: usize,
    pub relations_created: usize,
    pub entities: Vec<Entity>,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/km/workspaces/:ws_id/knowledge-graph
pub async fn get_knowledge_graph(
    State(state): State<AppState>,
    Path(ws_id): Path<Uuid>,
) -> Result<Json<KnowledgeGraph>, ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    state
        .km_store
        .get_workspace(workspace_id)
        .map_err(ApiError)?;

    let graph = state.km_store.get_knowledge_graph(workspace_id);
    Ok(Json(graph))
}

/// GET /api/km/workspaces/:ws_id/entities
pub async fn list_entities(
    State(state): State<AppState>,
    Path(ws_id): Path<Uuid>,
    Query(filter): Query<EntityFilter>,
) -> Result<Json<Vec<Entity>>, ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    state
        .km_store
        .get_workspace(workspace_id)
        .map_err(ApiError)?;

    let entities = if let Some(ref q) = filter.q {
        state.km_store.search_entities(workspace_id, q)
    } else {
        state.km_store.list_entities(workspace_id)
    };

    let entities = if let Some(ref entity_type) = filter.entity_type {
        entities
            .into_iter()
            .filter(|e| e.entity_type == *entity_type)
            .collect()
    } else {
        entities
    };

    Ok(Json(entities))
}

/// GET /api/km/workspaces/:ws_id/entities/:entity_id
pub async fn get_entity(
    State(state): State<AppState>,
    Path((ws_id, entity_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<EntityWithRelations>, ApiError> {
    let _workspace_id = WorkspaceId(ws_id);
    let eid = EntityId(entity_id);

    let entity = state.km_store.get_entity(eid).map_err(ApiError)?;
    let relations = state.km_store.get_entity_relations(eid);

    Ok(Json(EntityWithRelations { entity, relations }))
}

/// POST /api/km/workspaces/:ws_id/documents/:doc_id/extract
pub async fn extract_from_document(
    State(state): State<AppState>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<ExtractionResult>), ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    let doc_id = DocId(doc_id);

    if !state.config.knowledge_graph.enabled {
        return Err(ApiError(ThaiRagError::Validation(
            "Knowledge graph extraction is not enabled. Set knowledge_graph.enabled = true in config.".into(),
        )));
    }

    state
        .km_store
        .get_workspace(workspace_id)
        .map_err(ApiError)?;

    let content = state
        .km_store
        .get_document_content(doc_id)
        .map_err(ApiError)?
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Document has no content".into())))?;

    if content.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "Document content is empty".into(),
        )));
    }

    // Create LLM provider from current config
    let p = state.providers();
    let llm: Arc<dyn thairag_core::traits::LlmProvider> = Arc::from(
        thairag_provider_llm::create_llm_provider(&p.providers_config.llm),
    );

    let extracted_entities = extract_entities_from_text(&llm, &content).await;

    let mut created_entities = Vec::new();
    let mut entity_map = std::collections::HashMap::new();
    for (name, entity_type) in &extracted_entities {
        match state
            .km_store
            .upsert_entity(name, entity_type, workspace_id, serde_json::json!({}))
        {
            Ok(entity) => {
                let _ = state.km_store.add_entity_doc_link(entity.id, doc_id);
                entity_map.insert(name.clone(), entity.id);
                created_entities.push(entity);
            }
            Err(e) => {
                warn!("Failed to upsert entity '{}': {}", name, e);
            }
        }
    }

    let extracted_relations =
        extract_relations_from_text(&llm, &content, &extracted_entities).await;

    let mut relations_created = 0;
    for (from_name, to_name, rel_type, confidence) in &extracted_relations {
        if let (Some(&from_id), Some(&to_id)) = (entity_map.get(from_name), entity_map.get(to_name))
        {
            match state
                .km_store
                .insert_relation(from_id, to_id, rel_type, *confidence, doc_id)
            {
                Ok(_) => relations_created += 1,
                Err(e) => {
                    warn!(
                        "Failed to insert relation '{}' -> '{}': {}",
                        from_name, to_name, e
                    );
                }
            }
        }
    }

    info!(
        "Knowledge graph extraction complete: {} entities, {} relations from doc {}",
        created_entities.len(),
        relations_created,
        doc_id
    );

    Ok((
        StatusCode::CREATED,
        Json(ExtractionResult {
            entities_created: created_entities.len(),
            relations_created,
            entities: created_entities,
        }),
    ))
}

/// DELETE /api/km/workspaces/:ws_id/entities/:entity_id
pub async fn delete_entity(
    State(state): State<AppState>,
    Path((_ws_id, entity_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let eid = EntityId(entity_id);
    state.km_store.delete_entity(eid).map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
