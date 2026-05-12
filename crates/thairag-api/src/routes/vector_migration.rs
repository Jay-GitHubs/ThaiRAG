use std::sync::Arc;

use axum::extract::State;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use thairag_auth::AuthClaims;
use thairag_config::schema::VectorStoreConfig;
use thairag_core::types::VectorStoreKind;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::settings::require_super_admin;
use crate::vector_migration::{MigrationResult, MigrationState, MigrationStatus, ValidationResult};

// ── Request / Response types ────────────────────────────────────────

#[derive(Deserialize)]
pub struct StartMigrationRequest {
    pub target: TargetConfig,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_batch_size() -> usize {
    100
}

#[derive(Deserialize, Serialize)]
pub struct TargetConfig {
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub collection: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Serialize)]
pub struct StartMigrationResponse {
    pub message: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub state: String,
    pub total: usize,
    pub migrated: usize,
    pub failed: usize,
    pub target: Option<VectorStoreConfig>,
    pub result: Option<MigrationResult>,
    pub validation: Option<ValidationResult>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SwitchResponse {
    pub message: String,
    pub new_backend: String,
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /api/admin/vector-migration/start
pub async fn start_migration(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<StartMigrationRequest>,
) -> Result<Json<StartMigrationResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let migration_status = state.migration_status();

    // Check if already running
    {
        let s = migration_status.read().await;
        if s.state == MigrationState::Running {
            return Err(ApiError(thairag_core::ThaiRagError::Validation(
                "A migration is already in progress".into(),
            )));
        }
    }

    // Parse target kind
    let kind = parse_vector_store_kind(&body.target.kind)
        .map_err(|e| ApiError(thairag_core::ThaiRagError::Validation(e)))?;

    let target_config = VectorStoreConfig {
        kind,
        url: body.target.url.clone(),
        collection: body.target.collection.clone(),
        api_key: body.target.api_key.clone(),
        isolation: thairag_core::types::VectorIsolation::Shared,
    };

    let batch_size = body.batch_size.clamp(10, 1000);

    // Reset status
    {
        let mut s = migration_status.write().await;
        *s = MigrationStatus {
            state: MigrationState::Running,
            total: 0,
            migrated: 0,
            failed: 0,
            target_config: Some(target_config.clone()),
            result: None,
            validation: None,
            error: None,
        };
    }

    // Load all chunks from KM store (includes embeddings)
    let km_store = state.km_store.clone();
    let status_clone = migration_status.clone();

    // Run migration in background
    tokio::spawn(async move {
        // Load chunks synchronously from the KM store
        let chunks = km_store.load_all_chunks();

        let target_store = crate::vector_migration::create_target_store(&target_config);

        match crate::vector_migration::migrate_from_chunks(
            chunks,
            target_store.as_ref(),
            batch_size,
            status_clone.clone(),
        )
        .await
        {
            Ok(result) => {
                let mut s = status_clone.write().await;
                s.state = MigrationState::Completed;
                s.result = Some(result);
            }
            Err(e) => {
                let mut s = status_clone.write().await;
                s.state = MigrationState::Failed;
                s.error = Some(e.to_string());
            }
        }
    });

    Ok(Json(StartMigrationResponse {
        message: "Migration started in background".to_string(),
    }))
}

/// GET /api/admin/vector-migration/status
pub async fn get_migration_status(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<StatusResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let status = state.migration_status();
    let s = status.read().await;

    Ok(Json(StatusResponse {
        state: format!("{:?}", s.state).to_lowercase(),
        total: s.total,
        migrated: s.migrated,
        failed: s.failed,
        target: s.target_config.clone(),
        result: s.result.clone(),
        validation: s.validation.clone(),
        error: s.error.clone(),
    }))
}

/// POST /api/admin/vector-migration/validate
pub async fn validate_migration(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<ValidationResult>, ApiError> {
    require_super_admin(&claims, &state)?;

    let migration_status = state.migration_status();
    let target_config = {
        let s = migration_status.read().await;
        if s.state != MigrationState::Completed {
            return Err(ApiError(thairag_core::ThaiRagError::Validation(
                "No completed migration to validate. Run migration first.".into(),
            )));
        }
        s.target_config.clone().ok_or_else(|| {
            ApiError(thairag_core::ThaiRagError::Validation(
                "No target configuration found".into(),
            ))
        })?
    };

    // Count source vectors from current provider
    let providers = state.providers();
    let source_stats = providers.search_engine.vector_store_stats().await?;
    let source_count = source_stats.vector_count as usize;

    // Count target vectors
    let target_store = crate::vector_migration::create_target_store(&target_config);
    let target_stats = target_store.collection_stats().await?;
    let target_count = target_stats.vector_count as usize;

    let is_valid = target_count >= source_count;
    let message = if is_valid {
        format!("Validation passed: source={source_count}, target={target_count}")
    } else {
        format!("Count mismatch: source={source_count}, target={target_count}")
    };

    let result = ValidationResult {
        source_count,
        target_count,
        samples_checked: 0,
        samples_matched: 0,
        is_valid,
        message,
    };

    // Store validation result
    {
        let mut s = migration_status.write().await;
        s.validation = Some(result.clone());
        if is_valid {
            s.state = MigrationState::Validated;
        }
    }

    Ok(Json(result))
}

/// POST /api/admin/vector-migration/switch
pub async fn switch_provider(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<SwitchResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let migration_status = state.migration_status();
    let target_config = {
        let s = migration_status.read().await;
        if s.state != MigrationState::Validated && s.state != MigrationState::Completed {
            return Err(ApiError(thairag_core::ThaiRagError::Validation(
                "Migration must be completed (and ideally validated) before switching".into(),
            )));
        }
        s.target_config.clone().ok_or_else(|| {
            ApiError(thairag_core::ThaiRagError::Validation(
                "No target configuration found".into(),
            ))
        })?
    };

    let new_backend = format!("{:?}", target_config.kind).to_lowercase();

    // Update the provider config and persist to settings
    let mut providers_config = state.providers().providers_config.clone();
    providers_config.vector_store = target_config;

    state.km_store.set_setting(
        "providers.vector_store",
        &serde_json::to_string(&providers_config.vector_store).unwrap_or_default(),
    );

    // Rebuild providers with the new vector store config
    let bundle = crate::app_state::ProviderBundle::build_full_with_cache(
        &providers_config,
        &state.config.search,
        &state.config.document,
        &state.providers().chat_pipeline_config,
        state.prompt_registry.clone(),
        Some(state.km_store.clone()),
        Some(&state.vault),
        Some(state.embedding_cache.clone()),
        Some(
            Arc::clone(&state.plugin_registry) as Arc<dyn thairag_core::traits::SearchPluginEngine>
        ),
    );
    state.reload_providers(bundle);

    // Reset migration status
    {
        let mut s = migration_status.write().await;
        *s = MigrationStatus::default();
    }

    Ok(Json(SwitchResponse {
        message: format!("Switched to {new_backend} vector store"),
        new_backend,
    }))
}

fn parse_vector_store_kind(s: &str) -> std::result::Result<VectorStoreKind, String> {
    match s.to_lowercase().as_str() {
        "in_memory" | "inmemory" => Ok(VectorStoreKind::InMemory),
        "qdrant" => Ok(VectorStoreKind::Qdrant),
        "pgvector" => Ok(VectorStoreKind::Pgvector),
        "chromadb" | "chroma_db" => Ok(VectorStoreKind::ChromaDb),
        "pinecone" => Ok(VectorStoreKind::Pinecone),
        "weaviate" => Ok(VectorStoreKind::Weaviate),
        "milvus" => Ok(VectorStoreKind::Milvus),
        other => Err(format!("Unknown vector store kind: {other}")),
    }
}
