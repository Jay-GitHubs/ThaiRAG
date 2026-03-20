use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
use tracing::{info, instrument};

pub struct ChromaDbVectorStore {
    client: Client,
    url: String,
    collection_id: String,
}

impl ChromaDbVectorStore {
    pub fn new(url: &str, collection: &str) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client for ChromaDB");

        // Create or get collection synchronously during init
        let url = url.trim_end_matches('/').to_string();
        let collection_id = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let resp = client
                    .post(format!("{}/api/v1/collections", url))
                    .json(&json!({
                        "name": collection,
                        "get_or_create": true
                    }))
                    .send()
                    .await
                    .expect("Failed to create/get ChromaDB collection");

                let body: Value = resp
                    .json()
                    .await
                    .expect("Failed to parse ChromaDB collection response");

                body["id"]
                    .as_str()
                    .expect("ChromaDB collection response missing 'id'")
                    .to_string()
            })
        });

        info!(
            url,
            collection, collection_id, "Initialized ChromaDB vector store"
        );

        Self {
            client,
            url,
            collection_id,
        }
    }
}

#[async_trait]
impl VectorStore for ChromaDbVectorStore {
    #[instrument(skip(self, chunks), fields(collection_id = %self.collection_id, chunk_count = chunks.len()))]
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let mut ids = Vec::new();
        let mut documents = Vec::new();
        let mut embeddings = Vec::new();
        let mut metadatas = Vec::new();

        for chunk in chunks {
            let embedding = match chunk.embedding.as_ref() {
                Some(e) => e,
                None => continue,
            };

            ids.push(chunk.chunk_id.0.to_string());
            documents.push(chunk.content.clone());
            embeddings.push(embedding.clone());
            metadatas.push(json!({
                "doc_id": chunk.doc_id.0.to_string(),
                "workspace_id": chunk.workspace_id.0.to_string(),
                "chunk_index": chunk.chunk_index as i64,
            }));
        }

        if ids.is_empty() {
            return Ok(());
        }

        let resp = self
            .client
            .post(format!(
                "{}/api/v1/collections/{}/upsert",
                self.url, self.collection_id
            ))
            .json(&json!({
                "ids": ids,
                "documents": documents,
                "embeddings": embeddings,
                "metadatas": metadatas,
            }))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("ChromaDB upsert request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "ChromaDB upsert failed ({status}): {body}"
            )));
        }

        Ok(())
    }

    #[instrument(skip(self, embedding), fields(collection_id = %self.collection_id, top_k = query.top_k))]
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut body = json!({
            "query_embeddings": [embedding],
            "n_results": query.top_k,
            "include": ["documents", "metadatas", "distances"],
        });

        if !query.unrestricted {
            let ws_strings: Vec<String> = query
                .workspace_ids
                .iter()
                .map(|id| id.0.to_string())
                .collect();
            body["where"] = json!({
                "workspace_id": { "$in": ws_strings }
            });
        }

        let resp = self
            .client
            .post(format!(
                "{}/api/v1/collections/{}/query",
                self.url, self.collection_id
            ))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("ChromaDB search request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "ChromaDB search failed ({status}): {body}"
            )));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("ChromaDB search parse failed: {e}")))?;

        let mut results = Vec::new();

        // ChromaDB returns arrays of arrays (one per query embedding)
        let ids = body["ids"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.as_array());
        let documents = body["documents"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.as_array());
        let metadatas = body["metadatas"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.as_array());
        let distances = body["distances"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.as_array());

        if let (Some(ids), Some(documents), Some(metadatas), Some(distances)) =
            (ids, documents, metadatas, distances)
        {
            for i in 0..ids.len() {
                let chunk_id_str = match ids[i].as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let content = documents[i].as_str().unwrap_or_default().to_string();
                let meta = &metadatas[i];
                let distance = distances[i].as_f64().unwrap_or(1.0);

                let doc_id_str = meta["doc_id"].as_str().unwrap_or_default();
                let ws_id_str = meta["workspace_id"].as_str().unwrap_or_default();
                let chunk_index = meta["chunk_index"].as_u64().unwrap_or(0) as usize;

                let chunk_id = match chunk_id_str.parse() {
                    Ok(id) => ChunkId(id),
                    Err(_) => continue,
                };
                let doc_id = match doc_id_str.parse() {
                    Ok(id) => DocId(id),
                    Err(_) => continue,
                };
                let workspace_id = match ws_id_str.parse() {
                    Ok(id) => WorkspaceId(id),
                    Err(_) => continue,
                };

                // ChromaDB returns distance; convert to similarity score (1 - distance for L2,
                // or use directly if cosine distance). We assume cosine: score = 1 - distance.
                let score = 1.0 - distance as f32;

                results.push(SearchResult {
                    chunk: DocumentChunk {
                        chunk_id,
                        doc_id,
                        workspace_id,
                        content,
                        chunk_index,
                        embedding: None,
                        metadata: None,
                    },
                    score,
                });
            }
        }

        Ok(results)
    }

    #[instrument(skip(self), fields(collection_id = %self.collection_id, doc_id = %doc_id))]
    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/collections/{}/delete",
                self.url, self.collection_id
            ))
            .json(&json!({
                "where": { "doc_id": doc_id.0.to_string() }
            }))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("ChromaDB delete request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "ChromaDB delete failed ({status}): {body}"
            )));
        }

        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        // Delete the collection entirely — it will be recreated on next upsert via get_or_create
        let resp = self
            .client
            .delete(format!(
                "{}/api/v1/collections/{}",
                self.url, self.collection_id
            ))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("ChromaDB delete_all request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::VectorStore(format!(
                "ChromaDB delete_all failed ({status}): {body}"
            )));
        }

        info!(
            collection_id = %self.collection_id,
            "Deleted ChromaDB collection for re-indexing"
        );
        Ok(())
    }

    async fn collection_stats(&self) -> Result<thairag_core::types::VectorStoreStats> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/collections/{}/count",
                self.url, self.collection_id
            ))
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::VectorStore(format!("ChromaDB count request failed: {e}"))
            })?;
        let count = resp.json::<u64>().await.unwrap_or(0);
        Ok(thairag_core::types::VectorStoreStats {
            backend: "chromadb".to_string(),
            collection_name: self.collection_id.clone(),
            vector_count: count,
        })
    }
}
