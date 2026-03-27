use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thairag_config::schema::VectorStoreConfig;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::DocumentChunk;
use thairag_provider_vectordb::create_raw_vector_store;
use tokio::sync::RwLock;
use tracing::{error, info};

/// Result of a migration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub total: usize,
    pub migrated: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
}

/// Result of a validation check after migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub source_count: usize,
    pub target_count: usize,
    pub samples_checked: usize,
    pub samples_matched: usize,
    pub is_valid: bool,
    pub message: String,
}

/// Status of an ongoing or completed migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStatus {
    pub state: MigrationState,
    pub total: usize,
    pub migrated: usize,
    pub failed: usize,
    pub target_config: Option<VectorStoreConfig>,
    pub result: Option<MigrationResult>,
    pub validation: Option<ValidationResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    Idle,
    Running,
    Completed,
    Failed,
    Validated,
}

impl Default for MigrationStatus {
    fn default() -> Self {
        Self {
            state: MigrationState::Idle,
            total: 0,
            migrated: 0,
            failed: 0,
            target_config: None,
            result: None,
            validation: None,
            error: None,
        }
    }
}

/// Shared migration state accessible from route handlers.
pub type SharedMigrationStatus = Arc<RwLock<MigrationStatus>>;

/// Migrate all chunks (with embeddings) from the KM store into the target vector store.
/// This approach avoids needing to export from the current vector store directly;
/// chunks are the source of truth since they include the embeddings.
pub async fn migrate_from_chunks(
    chunks: Vec<DocumentChunk>,
    target: &dyn VectorStore,
    batch_size: usize,
    status: SharedMigrationStatus,
) -> Result<MigrationResult> {
    let start = Instant::now();

    // Filter to only chunks that have embeddings
    let chunks_with_embeddings: Vec<DocumentChunk> = chunks
        .into_iter()
        .filter(|c| c.embedding.is_some())
        .collect();
    let total = chunks_with_embeddings.len();

    {
        let mut s = status.write().await;
        s.total = total;
    }

    info!(
        total,
        "Migrating {total} chunks with embeddings to target vector store..."
    );

    let mut migrated = 0usize;
    let mut failed = 0usize;

    // Process in batches
    for batch in chunks_with_embeddings.chunks(batch_size) {
        match target.upsert(batch).await {
            Ok(()) => {
                migrated += batch.len();
            }
            Err(e) => {
                error!("Migration batch failed: {e}");
                failed += batch.len();
            }
        }

        // Update progress
        {
            let mut s = status.write().await;
            s.migrated = migrated;
            s.failed = failed;
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let result = MigrationResult {
        total,
        migrated,
        failed,
        skipped: 0,
        duration_ms,
    };

    info!(
        total,
        migrated, failed, duration_ms, "Vector migration completed"
    );

    Ok(result)
}

/// Create a target vector store from config, returning it as a boxed trait object.
pub fn create_target_store(config: &VectorStoreConfig) -> Box<dyn VectorStore> {
    create_raw_vector_store(config)
}
