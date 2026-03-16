use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
use tracing::{info, instrument};

pub struct MilvusVectorStore {
    client: Client,
    url: String,
    collection: String,
    collection_ready: AtomicBool,
}

impl MilvusVectorStore {
    pub fn new(url: &str, collection: &str) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client for Milvus");

        let url = url.trim_end_matches('/').to_string();

        info!(url, collection, "Initialized Milvus vector store");

        Self {
            client,
            url,
            collection: collection.to_string(),
            collection_ready: AtomicBool::new(false),
        }
    }

    async fn ensure_collection(&self, dim: usize) -> Result<()> {
        if self.collection_ready.load(Ordering::Relaxed) {
            return Ok(());
        }

        let resp = self
            .client
            .post(format!("{}/v2/vectordb/collections/create", self.url))
            .json(&json!({
                "collectionName": self.collection,
                "dimension": dim,
                "metricType": "COSINE",
            }))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Milvus create collection request failed: {e}"))
            })?;

        // Ignore "already exists" errors
        if resp.status().is_success() {
            let body: Value = resp.json().await.unwrap_or_default();
            let code = body["code"].as_i64().unwrap_or(0);
            // code 0 = success, code 65535 = already exists
            if code != 0 && code != 65535 {
                let msg = body["message"].as_str().unwrap_or("unknown error");
                return Err(ThaiRagError::VectorStore(format!(
                    "Milvus create collection failed: {msg}"
                )));
            }
        }

        self.collection_ready.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl VectorStore for MilvusVectorStore {
    #[instrument(skip(self, chunks), fields(collection = %self.collection, chunk_count = chunks.len()))]
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        // Determine dimension from the first chunk with an embedding
        if let Some(dim) = chunks
            .iter()
            .find_map(|c| c.embedding.as_ref().map(|e| e.len()))
        {
            self.ensure_collection(dim).await?;
        }

        let data: Vec<Value> = chunks
            .iter()
            .filter_map(|chunk| {
                let embedding = chunk.embedding.as_ref()?;
                Some(json!({
                    "id": chunk.chunk_id.0.to_string(),
                    "vector": embedding,
                    "doc_id": chunk.doc_id.0.to_string(),
                    "workspace_id": chunk.workspace_id.0.to_string(),
                    "content": chunk.content,
                    "chunk_index": chunk.chunk_index as i64,
                }))
            })
            .collect();

        if data.is_empty() {
            return Ok(());
        }

        let resp = self
            .client
            .post(format!("{}/v2/vectordb/entities/upsert", self.url))
            .json(&json!({
                "collectionName": self.collection,
                "data": data,
            }))
            .send()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Milvus upsert request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Milvus upsert failed ({status}): {body}"
            )));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Milvus upsert parse failed: {e}")))?;

        let code = body["code"].as_i64().unwrap_or(0);
        if code != 0 {
            let msg = body["message"].as_str().unwrap_or("unknown error");
            return Err(ThaiRagError::VectorStore(format!(
                "Milvus upsert failed: {msg}"
            )));
        }

        Ok(())
    }

    #[instrument(skip(self, embedding), fields(collection = %self.collection, top_k = query.top_k))]
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut body = json!({
            "collectionName": self.collection,
            "data": [embedding],
            "limit": query.top_k,
            "outputFields": ["doc_id", "workspace_id", "content", "chunk_index"],
        });

        if !query.unrestricted {
            let ws_list: Vec<String> = query
                .workspace_ids
                .iter()
                .map(|id| format!("'{}'", id.0))
                .collect();
            body["filter"] = json!(format!("workspace_id in [{}]", ws_list.join(",")));
        }

        let resp = self
            .client
            .post(format!("{}/v2/vectordb/entities/search", self.url))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Milvus search request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Milvus search failed ({status}): {body}"
            )));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Milvus search parse failed: {e}")))?;

        let code = body["code"].as_i64().unwrap_or(0);
        if code != 0 {
            let msg = body["message"].as_str().unwrap_or("unknown error");
            return Err(ThaiRagError::VectorStore(format!(
                "Milvus search failed: {msg}"
            )));
        }

        // Milvus returns data as array of arrays (one per query vector)
        let data = body["data"].as_array().cloned().unwrap_or_default();

        let results = data
            .into_iter()
            .filter_map(|item| {
                let chunk_id_str = item["id"].as_str()?;
                let doc_id_str = item["doc_id"].as_str()?;
                let ws_id_str = item["workspace_id"].as_str()?;
                let content = item["content"].as_str().unwrap_or_default().to_string();
                let chunk_index = item["chunk_index"].as_u64().unwrap_or(0) as usize;
                let score = item["distance"].as_f64().unwrap_or(0.0) as f32;

                Some(SearchResult {
                    chunk: DocumentChunk {
                        chunk_id: ChunkId(chunk_id_str.parse().ok()?),
                        doc_id: DocId(doc_id_str.parse().ok()?),
                        workspace_id: WorkspaceId(ws_id_str.parse().ok()?),
                        content,
                        chunk_index,
                        embedding: None,
                        metadata: None,
                    },
                    score,
                })
            })
            .collect();

        Ok(results)
    }

    #[instrument(skip(self), fields(collection = %self.collection, doc_id = %doc_id))]
    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/v2/vectordb/entities/delete", self.url))
            .json(&json!({
                "collectionName": self.collection,
                "filter": format!("doc_id == '{}'", doc_id.0),
            }))
            .send()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Milvus delete request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Milvus delete failed ({status}): {body}"
            )));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Milvus delete parse failed: {e}")))?;

        let code = body["code"].as_i64().unwrap_or(0);
        if code != 0 {
            let msg = body["message"].as_str().unwrap_or("unknown error");
            return Err(ThaiRagError::VectorStore(format!(
                "Milvus delete failed: {msg}"
            )));
        }

        Ok(())
    }
}
