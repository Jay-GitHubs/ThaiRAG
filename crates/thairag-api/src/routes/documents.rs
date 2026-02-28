use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use thairag_core::types::DocId;
use tracing::info;
use uuid::Uuid;

use crate::app_state::AppState;

#[derive(Deserialize)]
pub struct IngestRequest {
    pub title: String,
    pub content: String,
    #[serde(default = "default_mime")]
    pub mime_type: String,
}

fn default_mime() -> String {
    "text/plain".to_string()
}

#[derive(Serialize)]
pub struct IngestResponse {
    pub doc_id: Uuid,
    pub chunks: usize,
}

pub async fn ingest_document(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, (StatusCode, String)> {
    let workspace_id = thairag_core::types::WorkspaceId(workspace_id);
    let doc_id = DocId::new();

    info!(
        %doc_id,
        %workspace_id,
        title = %body.title,
        mime_type = %body.mime_type,
        "Ingesting document"
    );

    // Convert + chunk
    let chunks = state
        .document_pipeline
        .process(body.content.as_bytes(), &body.mime_type, doc_id, workspace_id)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;

    let chunk_count = chunks.len();

    // Embed + index into both vector store and Tantivy
    state
        .search_engine
        .index_chunks(&chunks)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(%doc_id, chunk_count, "Document ingested successfully");

    Ok(Json(IngestResponse {
        doc_id: doc_id.0,
        chunks: chunk_count,
    }))
}
