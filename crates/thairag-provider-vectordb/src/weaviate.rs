use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
use thairag_core::ThaiRagError;
use tracing::{info, instrument};

pub struct WeaviateVectorStore {
    client: Client,
    url: String,
    collection: String,
    api_key: String,
}

impl WeaviateVectorStore {
    pub fn new(url: &str, collection: &str, api_key: &str) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client for Weaviate");

        let url = url.trim_end_matches('/').to_string();

        // Ensure the class/schema exists
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut req = client.post(format!("{}/v1/schema", url)).json(&json!({
                    "class": collection,
                    "vectorizer": "none",
                    "properties": [
                        { "name": "doc_id", "dataType": ["text"] },
                        { "name": "workspace_id", "dataType": ["text"] },
                        { "name": "content", "dataType": ["text"] },
                        { "name": "chunk_index", "dataType": ["int"] },
                        { "name": "chunk_id", "dataType": ["text"] },
                    ]
                }));

                if !api_key.is_empty() {
                    req = req.header("Authorization", format!("Bearer {api_key}"));
                }

                // Ignore 422 (class already exists)
                let _ = req.send().await;
            })
        });

        info!(url, collection, "Initialized Weaviate vector store");

        Self {
            client,
            url,
            collection: collection.to_string(),
            api_key: api_key.to_string(),
        }
    }

    fn auth_header(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            req
        } else {
            req.header("Authorization", format!("Bearer {}", self.api_key))
        }
    }
}

#[async_trait]
impl VectorStore for WeaviateVectorStore {
    #[instrument(skip(self, chunks), fields(collection = %self.collection, chunk_count = chunks.len()))]
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        for chunk in chunks {
            let embedding = match chunk.embedding.as_ref() {
                Some(e) => e,
                None => continue,
            };

            // Use a deterministic UUID derived from chunk_id for idempotent upserts
            let weaviate_id = chunk.chunk_id.0;

            let body = json!({
                "class": self.collection,
                "id": weaviate_id.to_string(),
                "vector": embedding,
                "properties": {
                    "doc_id": chunk.doc_id.0.to_string(),
                    "workspace_id": chunk.workspace_id.0.to_string(),
                    "content": chunk.content,
                    "chunk_index": chunk.chunk_index as i64,
                    "chunk_id": chunk.chunk_id.0.to_string(),
                }
            });

            // Try PUT first (update), fall back to POST (create)
            let req = self.client.put(format!(
                "{}/v1/objects/{}/{}",
                self.url, self.collection, weaviate_id
            ));
            let req = self.auth_header(req);
            let resp = req
                .json(&body)
                .send()
                .await
                .map_err(|e| ThaiRagError::VectorStore(format!("Weaviate upsert request failed: {e}")))?;

            if !resp.status().is_success() {
                // If PUT fails (object doesn't exist), try POST
                let req = self
                    .client
                    .post(format!("{}/v1/objects", self.url));
                let req = self.auth_header(req);
                let resp = req
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        ThaiRagError::VectorStore(format!("Weaviate create request failed: {e}"))
                    })?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ThaiRagError::VectorStore(format!(
                        "Weaviate upsert failed ({status}): {body}"
                    )));
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(self, embedding), fields(collection = %self.collection, top_k = query.top_k))]
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        let vector_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let where_filter = if query.unrestricted {
            String::new()
        } else {
            let operands: Vec<String> = query
                .workspace_ids
                .iter()
                .map(|id| {
                    format!(
                        r#"{{ path: ["workspace_id"], operator: Equal, valueText: "{}" }}"#,
                        id.0
                    )
                })
                .collect();

            if operands.len() == 1 {
                format!(", where: {}", operands[0])
            } else {
                format!(
                    ", where: {{ operator: Or, operands: [{}] }}",
                    operands.join(", ")
                )
            }
        };

        let graphql = format!(
            r#"{{
                Get {{
                    {class}(
                        nearVector: {{ vector: {vector} }}
                        limit: {limit}
                        {where_filter}
                    ) {{
                        doc_id
                        workspace_id
                        content
                        chunk_index
                        chunk_id
                        _additional {{ id distance }}
                    }}
                }}
            }}"#,
            class = self.collection,
            vector = vector_str,
            limit = query.top_k,
            where_filter = where_filter,
        );

        let req = self
            .client
            .post(format!("{}/v1/graphql", self.url));
        let req = self.auth_header(req);
        let resp = req
            .json(&json!({ "query": graphql }))
            .send()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Weaviate search request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Weaviate search failed ({status}): {body}"
            )));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Weaviate search parse failed: {e}")))?;

        let items = body["data"]["Get"][&self.collection]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let results = items
            .into_iter()
            .filter_map(|item| {
                let doc_id_str = item["doc_id"].as_str()?;
                let ws_id_str = item["workspace_id"].as_str()?;
                let content = item["content"].as_str().unwrap_or_default().to_string();
                let chunk_index = item["chunk_index"].as_u64().unwrap_or(0) as usize;
                let chunk_id_str = item["chunk_id"].as_str()?;
                let distance = item["_additional"]["distance"].as_f64().unwrap_or(1.0);

                // Weaviate cosine distance: score = 1 - distance
                let score = 1.0 - distance as f32;

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
        // Use batch delete with where filter
        let body = json!({
            "match": {
                "class": self.collection,
                "where": {
                    "path": ["doc_id"],
                    "operator": "Equal",
                    "valueText": doc_id.0.to_string()
                }
            }
        });

        let req = self
            .client
            .delete(format!("{}/v1/batch/objects", self.url));
        let req = self.auth_header(req);
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Weaviate delete request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "Weaviate delete failed ({status}): {body}"
            )));
        }

        Ok(())
    }
}
