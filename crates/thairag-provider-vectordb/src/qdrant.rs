use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, PointStruct,
    QueryPointsBuilder, ScrollPointsBuilder, UpsertPointsBuilder, Value, VectorParamsBuilder,
    point_id::PointIdOptions, value::Kind,
};
use qdrant_client::{Payload, Qdrant};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::{VectorStore, VectorStoreExport};
use thairag_core::types::{DocId, DocumentChunk, ExportedVector, SearchQuery, SearchResult};
use tracing::{info, instrument, warn};

/// Borrow a payload value as a `&str`. Returns `None` for non-string values.
/// Avoids `Value`'s `Display`, which debug-escapes strings (`{:?}`) and would
/// mangle multi-byte text such as Thai combining marks into `\u{...}` escapes.
fn value_as_str(value: &Value) -> Option<&str> {
    match value.kind.as_ref()? {
        Kind::StringValue(s) => Some(s),
        _ => None,
    }
}

/// Render a payload value as a clean owned `String` (no debug escaping).
fn value_to_string(value: &Value) -> String {
    match value.kind.as_ref() {
        Some(Kind::StringValue(s)) => s.clone(),
        Some(Kind::IntegerValue(i)) => i.to_string(),
        Some(Kind::DoubleValue(d)) => d.to_string(),
        Some(Kind::BoolValue(b)) => b.to_string(),
        _ => String::new(),
    }
}

pub struct QdrantVectorStore {
    client: Qdrant,
    collection: String,
    collection_ready: AtomicBool,
}

impl QdrantVectorStore {
    pub fn new(url: &str, collection: &str) -> Self {
        let client = Qdrant::from_url(url)
            .build()
            .expect("Failed to build Qdrant client");

        info!(url, collection, "Initialized Qdrant vector store");

        Self {
            client,
            collection: collection.to_string(),
            collection_ready: AtomicBool::new(false),
        }
    }

    async fn ensure_collection(&self, dimension: usize) -> Result<()> {
        if self.collection_ready.load(Ordering::Relaxed) {
            return Ok(());
        }

        let exists = self
            .client
            .collection_exists(&self.collection)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Failed to check collection: {e}")))?;

        if exists {
            // Check if existing collection dimension matches the requested dimension.
            // If the user switched embedding models the sizes will differ, causing silent
            // failures on upsert/search.  Recreate the collection in that case.
            let needs_recreate = match self.client.collection_info(&self.collection).await {
                Ok(info) => {
                    let existing_dim = info
                        .result
                        .and_then(|r| r.config)
                        .and_then(|c| c.params)
                        .and_then(|p| p.vectors_config)
                        .and_then(|vc| match vc.config {
                            Some(qdrant_client::qdrant::vectors_config::Config::Params(params)) => {
                                Some(params.size)
                            }
                            _ => None,
                        })
                        .unwrap_or(0);
                    existing_dim != dimension as u64
                }
                Err(_) => false,
            };

            if needs_recreate {
                warn!(
                    collection = %self.collection,
                    expected_dim = dimension,
                    "Qdrant collection has wrong vector dimension, recreating \
                     (existing vectors are incompatible with the current embedding model)"
                );
                self.client
                    .delete_collection(&self.collection)
                    .await
                    .map_err(|e| {
                        ThaiRagError::VectorStore(format!("Failed to delete collection: {e}"))
                    })?;
                self.client
                    .create_collection(
                        CreateCollectionBuilder::new(&self.collection).vectors_config(
                            VectorParamsBuilder::new(dimension as u64, Distance::Cosine),
                        ),
                    )
                    .await
                    .map_err(|e| {
                        ThaiRagError::VectorStore(format!("Failed to recreate collection: {e}"))
                    })?;
                info!(collection = %self.collection, dimension, "Recreated Qdrant collection with new dimension");
            }
        } else {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection).vectors_config(
                        VectorParamsBuilder::new(dimension as u64, Distance::Cosine),
                    ),
                )
                .await
                .map_err(|e| {
                    ThaiRagError::VectorStore(format!("Failed to create collection: {e}"))
                })?;

            info!(collection = %self.collection, dimension, "Created Qdrant collection");
        }

        self.collection_ready.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    #[instrument(skip(self, chunks), fields(collection = %self.collection, chunk_count = chunks.len()))]
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        // Determine dimension from the first chunk that has an embedding
        if let Some(dim) = chunks
            .iter()
            .find_map(|c| c.embedding.as_ref().map(|e| e.len()))
        {
            self.ensure_collection(dim).await?;
        }

        let points: Vec<PointStruct> = chunks
            .iter()
            .filter_map(|chunk| {
                let embedding = chunk.embedding.as_ref()?;
                let payload = Payload::try_from(serde_json::json!({
                    "doc_id": chunk.doc_id.to_string(),
                    "workspace_id": chunk.workspace_id.to_string(),
                    "content": chunk.content,
                    "chunk_index": chunk.chunk_index as i64,
                }))
                .ok()?;

                Some(PointStruct::new(
                    chunk.chunk_id.to_string(),
                    embedding.clone(),
                    payload,
                ))
            })
            .collect();

        if points.is_empty() {
            return Ok(());
        }

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, points).wait(true))
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Qdrant upsert failed: {e}")))?;

        Ok(())
    }

    #[instrument(skip(self, embedding), fields(collection = %self.collection, top_k = query.top_k))]
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // No access: not unrestricted and no workspace permissions
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        // The readiness flag is only primed by `ensure_collection` during
        // indexing. On a fresh process the collection may already exist in
        // Qdrant (e.g. after a restart/rebuild with persisted data), so probe
        // before giving up — otherwise vector search stays silently dead until
        // the next ingest, and queries fall back to BM25 only.
        if !self.collection_ready.load(Ordering::Relaxed) {
            match self.client.collection_exists(&self.collection).await {
                Ok(true) => self.collection_ready.store(true, Ordering::Relaxed),
                Ok(false) => return Ok(vec![]),
                Err(e) => {
                    return Err(ThaiRagError::VectorStore(format!(
                        "Failed to check collection: {e}"
                    )));
                }
            }
        }

        let mut request = QueryPointsBuilder::new(&self.collection)
            .query(embedding.to_vec())
            .limit(query.top_k as u64)
            .with_payload(true);

        // Apply workspace + optional document filters. doc_ids restricts
        // retrieval to a specific set of documents (agentic doc-selection).
        let mut conditions: Vec<Condition> = Vec::new();
        if !query.unrestricted && !query.workspace_ids.is_empty() {
            let workspace_strings: Vec<String> = query
                .workspace_ids
                .iter()
                .map(|id| id.to_string())
                .collect();
            conditions.push(Condition::matches("workspace_id", workspace_strings));
        }
        if !query.doc_ids.is_empty() {
            let doc_strings: Vec<String> = query.doc_ids.iter().map(|id| id.to_string()).collect();
            conditions.push(Condition::matches("doc_id", doc_strings));
        }
        if !conditions.is_empty() {
            request = request.filter(Filter::must(conditions));
        }

        let response = self
            .client
            .query(request)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Qdrant search failed: {e}")))?;

        let results = response
            .result
            .into_iter()
            .filter_map(|point| {
                let score = point.score;

                let payload = &point.payload;
                let doc_id_str = payload.get("doc_id").and_then(value_as_str)?;
                let workspace_id_str = payload.get("workspace_id").and_then(value_as_str)?;
                let content = payload
                    .get("content")
                    .and_then(value_as_str)
                    .unwrap_or_default();
                let chunk_index = payload
                    .get("chunk_index")
                    .map(value_to_string)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);

                let chunk_id_str = match point.id.as_ref()?.point_id_options.as_ref()? {
                    PointIdOptions::Uuid(uuid) => uuid.clone(),
                    PointIdOptions::Num(n) => n.to_string(),
                };

                let chunk = DocumentChunk {
                    chunk_id: thairag_core::types::ChunkId(chunk_id_str.parse().ok()?),
                    doc_id: DocId(doc_id_str.parse().ok()?),
                    workspace_id: thairag_core::types::WorkspaceId(workspace_id_str.parse().ok()?),
                    content: content.to_string(),
                    chunk_index,
                    embedding: None,
                    metadata: None,
                };

                Some(SearchResult { chunk, score })
            })
            .collect();

        Ok(results)
    }

    #[instrument(skip(self), fields(collection = %self.collection, doc_id = %doc_id))]
    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection)
                    .points(Filter::must([Condition::matches(
                        "doc_id",
                        doc_id.to_string(),
                    )]))
                    .wait(true),
            )
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Qdrant delete failed: {e}")))?;

        Ok(())
    }

    #[instrument(skip(self), fields(collection = %self.collection))]
    async fn delete_all(&self) -> Result<()> {
        let exists = self
            .client
            .collection_exists(&self.collection)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Failed to check collection: {e}")))?;

        if exists {
            self.client
                .delete_collection(&self.collection)
                .await
                .map_err(|e| {
                    ThaiRagError::VectorStore(format!("Failed to delete collection: {e}"))
                })?;
            info!(collection = %self.collection, "Deleted Qdrant collection for re-indexing");
        }
        // Reset ready flag so collection gets recreated with new dimension
        self.collection_ready.store(false, Ordering::Relaxed);
        Ok(())
    }

    async fn collection_stats(&self) -> Result<thairag_core::types::VectorStoreStats> {
        let count = match self.client.collection_info(&self.collection).await {
            Ok(info) => info.result.map_or(0, |r| r.points_count.unwrap_or(0)),
            Err(_) => 0,
        };
        Ok(thairag_core::types::VectorStoreStats {
            backend: "qdrant".to_string(),
            collection_name: self.collection.clone(),
            vector_count: count,
        })
    }
}

#[async_trait]
impl VectorStoreExport for QdrantVectorStore {
    async fn export_all(&self, batch_size: usize) -> Result<Vec<ExportedVector>> {
        let exists = self
            .client
            .collection_exists(&self.collection)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Failed to check collection: {e}")))?;

        if !exists {
            return Ok(vec![]);
        }

        let mut all_vectors = Vec::new();
        let mut offset: Option<qdrant_client::qdrant::PointId> = None;

        loop {
            let mut scroll = ScrollPointsBuilder::new(&self.collection)
                .limit(batch_size as u32)
                .with_payload(true)
                .with_vectors(true);

            if let Some(ref off) = offset {
                scroll = scroll.offset(off.clone());
            }

            let response = self
                .client
                .scroll(scroll)
                .await
                .map_err(|e| ThaiRagError::VectorStore(format!("Qdrant scroll failed: {e}")))?;

            let points = &response.result;
            if points.is_empty() {
                break;
            }

            for point in points {
                let id = match point
                    .id
                    .as_ref()
                    .and_then(|pid| pid.point_id_options.as_ref())
                {
                    Some(PointIdOptions::Uuid(uuid)) => uuid.clone(),
                    Some(PointIdOptions::Num(n)) => n.to_string(),
                    None => continue,
                };

                // Extract embedding from vectors
                #[allow(deprecated)]
                let embedding = match &point.vectors {
                    Some(vectors) => {
                        use qdrant_client::qdrant::vectors_output::VectorsOptions;
                        match &vectors.vectors_options {
                            Some(VectorsOptions::Vector(v)) => v.data.clone(),
                            _ => continue,
                        }
                    }
                    None => continue,
                };

                let mut metadata = std::collections::HashMap::new();
                for (key, value) in &point.payload {
                    metadata.insert(key.clone(), value_to_string(value));
                }

                all_vectors.push(ExportedVector {
                    id,
                    embedding,
                    metadata,
                });
            }

            // Check if there's a next page offset
            match response.next_page_offset {
                Some(next_offset) => offset = Some(next_offset),
                None => break,
            }
        }

        Ok(all_vectors)
    }

    async fn count(&self) -> Result<usize> {
        let stats = self.collection_stats().await?;
        Ok(stats.vector_count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_value(s: &str) -> Value {
        Value {
            kind: Some(Kind::StringValue(s.to_string())),
        }
    }

    #[test]
    fn value_as_str_preserves_thai_combining_marks() {
        // Regression: Value's Display impl escapes strings with `{:?}`, turning
        // Thai vowels/tone marks into `\u{...}`. Reading payload via the helper
        // must return the raw UTF-8 text untouched.
        let thai = "เกษตรพื้นฐาน ธุรกิจต้องห้าม";
        let value = str_value(thai);
        assert_eq!(value_as_str(&value), Some(thai));
        assert_eq!(value_to_string(&value), thai);
        // The buggy path (Display) would have escaped the combining marks.
        assert!(value.to_string().contains("\\u{"));
    }

    #[test]
    fn value_helpers_handle_non_string_kinds() {
        let int_val = Value {
            kind: Some(Kind::IntegerValue(7)),
        };
        assert_eq!(value_as_str(&int_val), None);
        assert_eq!(value_to_string(&int_val), "7");
    }
}
