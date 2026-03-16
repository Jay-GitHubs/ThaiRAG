use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, PointStruct,
    QueryPointsBuilder, ScrollPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::PersonalMemoryStore;
use thairag_core::types::{MemoryId, PersonalMemory, PersonalMemoryType, UserId};
use tracing::{info, instrument};

const COLLECTION_NAME: &str = "thairag_personal_memory";

pub struct QdrantPersonalMemoryStore {
    client: Qdrant,
    dimension: usize,
    collection_ready: AtomicBool,
}

impl QdrantPersonalMemoryStore {
    pub fn new(url: &str, dimension: usize) -> Self {
        let client = Qdrant::from_url(url)
            .build()
            .expect("Failed to build Qdrant client for personal memory");

        info!(url, "Initialized Qdrant personal memory store");

        Self {
            client,
            dimension,
            collection_ready: AtomicBool::new(false),
        }
    }

    async fn ensure_collection(&self) -> Result<()> {
        if self.collection_ready.load(Ordering::Relaxed) {
            return Ok(());
        }

        let exists = self
            .client
            .collection_exists(COLLECTION_NAME)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Failed to check collection: {e}")))?;

        if !exists {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(COLLECTION_NAME).vectors_config(
                        VectorParamsBuilder::new(self.dimension as u64, Distance::Cosine),
                    ),
                )
                .await
                .map_err(|e| {
                    ThaiRagError::VectorStore(format!("Failed to create collection: {e}"))
                })?;

            info!(
                collection = COLLECTION_NAME,
                dimension = self.dimension,
                "Created personal memory collection"
            );
        }

        self.collection_ready.store(true, Ordering::Relaxed);
        Ok(())
    }
}

fn memory_type_to_string(mt: &PersonalMemoryType) -> &'static str {
    match mt {
        PersonalMemoryType::Preference => "preference",
        PersonalMemoryType::Fact => "fact",
        PersonalMemoryType::Decision => "decision",
        PersonalMemoryType::Conversation => "conversation",
        PersonalMemoryType::Correction => "correction",
    }
}

fn string_to_memory_type(s: &str) -> PersonalMemoryType {
    match s {
        "preference" => PersonalMemoryType::Preference,
        "fact" => PersonalMemoryType::Fact,
        "decision" => PersonalMemoryType::Decision,
        "correction" => PersonalMemoryType::Correction,
        _ => PersonalMemoryType::Conversation,
    }
}

#[async_trait]
impl PersonalMemoryStore for QdrantPersonalMemoryStore {
    #[instrument(skip(self, memory, embedding), fields(memory_id = %memory.id))]
    async fn store(&self, memory: &PersonalMemory, embedding: Vec<f32>) -> Result<()> {
        self.ensure_collection().await?;

        let topics_json = serde_json::to_string(&memory.topics).unwrap_or_default();

        let payload = Payload::try_from(serde_json::json!({
            "user_id": memory.user_id.to_string(),
            "memory_type": memory_type_to_string(&memory.memory_type),
            "summary": memory.summary,
            "topics": topics_json,
            "importance": memory.importance,
            "created_at": memory.created_at,
            "last_accessed_at": memory.last_accessed_at,
            "relevance_score": memory.relevance_score,
        }))
        .map_err(|e| ThaiRagError::VectorStore(format!("Failed to build payload: {e}")))?;

        let point = PointStruct::new(memory.id.to_string(), embedding, payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(COLLECTION_NAME, vec![point]).wait(true))
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Personal memory upsert failed: {e}"))
            })?;

        Ok(())
    }

    #[instrument(skip(self, query_embedding), fields(user_id = %user_id, top_k))]
    async fn search(
        &self,
        user_id: UserId,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<PersonalMemory>> {
        self.ensure_collection().await?;

        let request = QueryPointsBuilder::new(COLLECTION_NAME)
            .query(query_embedding.to_vec())
            .limit(top_k as u64)
            .with_payload(true)
            .filter(Filter::must([Condition::matches(
                "user_id",
                user_id.to_string(),
            )]));

        let response = self.client.query(request).await.map_err(|e| {
            ThaiRagError::VectorStore(format!("Personal memory search failed: {e}"))
        })?;

        let results = response
            .result
            .into_iter()
            .filter_map(|point| {
                let payload = &point.payload;
                let memory_id_str =
                    point
                        .id
                        .as_ref()?
                        .point_id_options
                        .as_ref()
                        .map(|opt| match opt {
                            qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u) => u.clone(),
                            qdrant_client::qdrant::point_id::PointIdOptions::Num(n) => {
                                n.to_string()
                            }
                        })?;

                let summary = payload
                    .get("summary")?
                    .to_string()
                    .trim_matches('"')
                    .to_string();
                let memory_type = payload
                    .get("memory_type")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .unwrap_or_default();
                let topics_str = payload
                    .get("topics")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .unwrap_or_default();
                let topics: Vec<String> = serde_json::from_str(&topics_str).unwrap_or_default();
                let importance = payload
                    .get("importance")
                    .and_then(|v| v.to_string().parse::<f32>().ok())
                    .unwrap_or(0.5);
                let created_at = payload
                    .get("created_at")
                    .and_then(|v| v.to_string().parse::<i64>().ok())
                    .unwrap_or(0);
                let last_accessed_at = payload
                    .get("last_accessed_at")
                    .and_then(|v| v.to_string().parse::<i64>().ok())
                    .unwrap_or(0);
                let relevance_score = payload
                    .get("relevance_score")
                    .and_then(|v| v.to_string().parse::<f32>().ok())
                    .unwrap_or(1.0);

                Some(PersonalMemory {
                    id: MemoryId(memory_id_str.parse().ok()?),
                    user_id,
                    memory_type: string_to_memory_type(&memory_type),
                    summary,
                    topics,
                    importance,
                    created_at,
                    last_accessed_at,
                    relevance_score,
                })
            })
            .collect();

        Ok(results)
    }

    async fn delete(&self, id: MemoryId) -> Result<()> {
        self.ensure_collection().await?;

        let point_id: qdrant_client::qdrant::PointId = id.to_string().into();
        self.client
            .delete_points(
                DeletePointsBuilder::new(COLLECTION_NAME)
                    .points(vec![point_id])
                    .wait(true),
            )
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Personal memory delete failed: {e}"))
            })?;

        Ok(())
    }

    async fn delete_all_for_user(&self, user_id: UserId) -> Result<()> {
        self.ensure_collection().await?;

        self.client
            .delete_points(
                DeletePointsBuilder::new(COLLECTION_NAME)
                    .points(Filter::must([Condition::matches(
                        "user_id",
                        user_id.to_string(),
                    )]))
                    .wait(true),
            )
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!(
                    "Personal memory delete_all_for_user failed: {e}"
                ))
            })?;

        Ok(())
    }

    async fn apply_decay(&self, decay_factor: f32, min_score: f32) -> Result<usize> {
        self.ensure_collection().await?;

        // Scroll through all points and update relevance scores
        let scroll = ScrollPointsBuilder::new(COLLECTION_NAME)
            .limit(1000)
            .with_payload(true);

        let response = self.client.scroll(scroll).await.map_err(|e| {
            ThaiRagError::VectorStore(format!("Personal memory scroll failed: {e}"))
        })?;

        let mut to_delete = Vec::new();
        let mut to_update = Vec::new();

        for point in &response.result {
            let current_score = point
                .payload
                .get("relevance_score")
                .and_then(|v| v.to_string().parse::<f32>().ok())
                .unwrap_or(1.0);

            let new_score = current_score * decay_factor;

            if new_score < min_score {
                if let Some(id) = &point.id {
                    to_delete.push(id.clone());
                }
            } else {
                // Update the score via upsert with vectors
                // For simplicity, we track IDs to update
                if let Some(id) = &point.id {
                    to_update.push((id.clone(), new_score));
                }
            }
        }

        let pruned = to_delete.len();

        // Delete low-relevance entries
        if !to_delete.is_empty() {
            self.client
                .delete_points(
                    DeletePointsBuilder::new(COLLECTION_NAME)
                        .points(to_delete)
                        .wait(true),
                )
                .await
                .map_err(|e| {
                    ThaiRagError::VectorStore(format!("Personal memory decay delete failed: {e}"))
                })?;
        }

        // Note: Updating payload scores would require set_payload which we skip for now.
        // The in-memory store handles this more cleanly. For Qdrant production use,
        // a dedicated maintenance task would handle score updates via set_payload.

        Ok(pruned)
    }
}
