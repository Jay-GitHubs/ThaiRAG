use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
use tracing::{info, instrument};

pub struct PineconeVectorStore {
    client: Client,
    url: String,
    api_key: String,
}

impl PineconeVectorStore {
    pub fn new(url: &str, api_key: &str) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client for Pinecone");

        let url = url.trim_end_matches('/').to_string();

        info!(url, "Initialized Pinecone vector store");

        Self {
            client,
            url,
            api_key: api_key.to_string(),
        }
    }
}

#[async_trait]
impl VectorStore for PineconeVectorStore {
    #[instrument(skip(self, chunks), fields(chunk_count = chunks.len()))]
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let vectors: Vec<Value> = chunks
            .iter()
            .filter_map(|chunk| {
                let embedding = chunk.embedding.as_ref()?;
                Some(json!({
                    "id": chunk.chunk_id.0.to_string(),
                    "values": embedding,
                    "metadata": {
                        "doc_id": chunk.doc_id.0.to_string(),
                        "workspace_id": chunk.workspace_id.0.to_string(),
                        "content": chunk.content,
                        "chunk_index": chunk.chunk_index as i64,
                    }
                }))
            })
            .collect();

        if vectors.is_empty() {
            return Ok(());
        }

        let resp = self
            .client
            .post(format!("{}/vectors/upsert", self.url))
            .header("Api-Key", &self.api_key)
            .json(&json!({ "vectors": vectors }))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Pinecone upsert request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Pinecone upsert failed ({status}): {body}"
            )));
        }

        Ok(())
    }

    #[instrument(skip(self, embedding), fields(top_k = query.top_k))]
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut body = json!({
            "vector": embedding,
            "topK": query.top_k,
            "includeMetadata": true,
        });

        if !query.unrestricted {
            let ws_strings: Vec<String> = query
                .workspace_ids
                .iter()
                .map(|id| id.0.to_string())
                .collect();
            body["filter"] = json!({
                "workspace_id": { "$in": ws_strings }
            });
        }

        let resp = self
            .client
            .post(format!("{}/query", self.url))
            .header("Api-Key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Pinecone search request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Pinecone search failed ({status}): {body}"
            )));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Pinecone search parse failed: {e}")))?;

        let matches = body["matches"].as_array();

        let results = matches
            .map(|matches| {
                matches
                    .iter()
                    .filter_map(|m| {
                        let chunk_id_str = m["id"].as_str()?;
                        let score = m["score"].as_f64()? as f32;
                        let meta = &m["metadata"];

                        let doc_id_str = meta["doc_id"].as_str()?;
                        let ws_id_str = meta["workspace_id"].as_str()?;
                        let content = meta["content"].as_str().unwrap_or_default().to_string();
                        let chunk_index = meta["chunk_index"].as_u64().unwrap_or(0) as usize;

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
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }

    #[instrument(skip(self), fields(doc_id = %doc_id))]
    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/vectors/delete", self.url))
            .header("Api-Key", &self.api_key)
            .json(&json!({
                "filter": { "doc_id": { "$eq": doc_id.0.to_string() } }
            }))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Pinecone delete request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Pinecone delete failed ({status}): {body}"
            )));
        }

        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/vectors/delete", self.url))
            .header("Api-Key", &self.api_key)
            .json(&json!({ "deleteAll": true }))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Pinecone delete_all request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Pinecone delete_all failed ({status}): {body}"
            )));
        }

        info!("Deleted all Pinecone vectors for re-indexing");
        Ok(())
    }

    async fn collection_stats(&self) -> Result<thairag_core::types::VectorStoreStats> {
        let resp = self
            .client
            .get(format!("{}/describe_index_stats", self.url))
            .header("Api-Key", &self.api_key)
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("Pinecone stats request failed: {e}"))
            })?;
        let body: Value = resp.json().await.unwrap_or_default();
        let count = body["totalVectorCount"].as_u64().unwrap_or(0);
        Ok(thairag_core::types::VectorStoreStats {
            backend: "pinecone".to_string(),
            collection_name: "default".to_string(),
            vector_count: count,
        })
    }
}
