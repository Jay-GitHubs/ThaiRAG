use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{DocId, DocumentChunk, SearchQuery, SearchResult};

/// In-memory vector store for development and testing.
/// Stores chunks and performs brute-force cosine similarity search.
pub struct InMemoryVectorStore {
    chunks: RwLock<HashMap<String, DocumentChunk>>,
}

impl InMemoryVectorStore {
    pub fn new() -> Self {
        Self {
            chunks: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        let mut store = self.chunks.write().unwrap();
        for chunk in chunks {
            store.insert(chunk.chunk_id.to_string(), chunk.clone());
        }
        Ok(())
    }

    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let store = self.chunks.read().unwrap();
        let mut results: Vec<SearchResult> = store
            .values()
            .filter(|chunk| query.unrestricted || query.workspace_ids.contains(&chunk.workspace_id))
            .filter_map(|chunk| {
                chunk.embedding.as_ref().map(|emb| {
                    let score = cosine_similarity(embedding, emb);
                    SearchResult {
                        chunk: chunk.clone(),
                        score,
                    }
                })
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(query.top_k);
        Ok(results)
    }

    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        let mut store = self.chunks.write().unwrap();
        store.retain(|_, chunk| chunk.doc_id != doc_id);
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let mut store = self.chunks.write().unwrap();
        store.clear();
        Ok(())
    }

    async fn collection_stats(&self) -> Result<thairag_core::types::VectorStoreStats> {
        let store = self.chunks.read().unwrap();
        Ok(thairag_core::types::VectorStoreStats {
            backend: "in_memory".to_string(),
            collection_name: "in_memory".to_string(),
            vector_count: store.len() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{ChunkId, WorkspaceId};
    use uuid::Uuid;

    fn make_chunk(id: &str, doc_id: DocId, ws_id: WorkspaceId, emb: Vec<f32>) -> DocumentChunk {
        DocumentChunk {
            chunk_id: ChunkId(Uuid::parse_str(id).unwrap()),
            doc_id,
            workspace_id: ws_id,
            content: format!("content-{id}"),
            chunk_index: 0,
            embedding: Some(emb),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn upsert_and_search() {
        let store = InMemoryVectorStore::new();
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();

        let chunk = make_chunk(
            "00000000-0000-0000-0000-000000000001",
            doc_id,
            ws_id,
            vec![1.0, 0.0, 0.0],
        );
        store.upsert(&[chunk]).await.unwrap();

        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let results = store.search(&[1.0, 0.0, 0.0], &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!((results[0].score - 1.0).abs() < 1e-6); // perfect cosine match
    }

    #[tokio::test]
    async fn cosine_ordering() {
        let store = InMemoryVectorStore::new();
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();

        let close = make_chunk(
            "00000000-0000-0000-0000-000000000001",
            doc_id,
            ws_id,
            vec![0.9, 0.1, 0.0],
        );
        let far = make_chunk(
            "00000000-0000-0000-0000-000000000002",
            doc_id,
            ws_id,
            vec![0.0, 0.0, 1.0],
        );
        store.upsert(&[close, far]).await.unwrap();

        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let results = store.search(&[1.0, 0.0, 0.0], &query).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].score > results[1].score);
    }

    #[tokio::test]
    async fn workspace_filter() {
        let store = InMemoryVectorStore::new();
        let doc_id = DocId::new();
        let ws_a = WorkspaceId::new();
        let ws_b = WorkspaceId::new();

        let chunk_a = make_chunk(
            "00000000-0000-0000-0000-000000000001",
            doc_id,
            ws_a,
            vec![1.0, 0.0, 0.0],
        );
        let chunk_b = make_chunk(
            "00000000-0000-0000-0000-000000000002",
            doc_id,
            ws_b,
            vec![1.0, 0.0, 0.0],
        );
        store.upsert(&[chunk_a, chunk_b]).await.unwrap();

        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 10,
            workspace_ids: vec![ws_a],
            unrestricted: false,
        };
        let results = store.search(&[1.0, 0.0, 0.0], &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.workspace_id, ws_a);
    }

    #[tokio::test]
    async fn top_k_limit() {
        let store = InMemoryVectorStore::new();
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();

        for i in 0..5u8 {
            let id = format!("00000000-0000-0000-0000-0000000000{:02x}", i + 1);
            let chunk = make_chunk(&id, doc_id, ws_id, vec![1.0, 0.0, 0.0]);
            store.upsert(&[chunk]).await.unwrap();
        }

        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 2,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let results = store.search(&[1.0, 0.0, 0.0], &query).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn delete_by_doc_removes_chunks() {
        let store = InMemoryVectorStore::new();
        let doc_a = DocId::new();
        let doc_b = DocId::new();
        let ws_id = WorkspaceId::new();

        let chunk_a = make_chunk(
            "00000000-0000-0000-0000-000000000001",
            doc_a,
            ws_id,
            vec![1.0, 0.0, 0.0],
        );
        let chunk_b = make_chunk(
            "00000000-0000-0000-0000-000000000002",
            doc_b,
            ws_id,
            vec![1.0, 0.0, 0.0],
        );
        store.upsert(&[chunk_a, chunk_b]).await.unwrap();

        store.delete_by_doc(doc_a).await.unwrap();

        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let results = store.search(&[1.0, 0.0, 0.0], &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.doc_id, doc_b);
    }

    #[tokio::test]
    async fn search_empty_store() {
        let store = InMemoryVectorStore::new();
        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let results = store.search(&[1.0, 0.0, 0.0], &query).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let store = InMemoryVectorStore::new();
        let doc_id = DocId::new();
        let ws_id = WorkspaceId::new();
        let id = "00000000-0000-0000-0000-000000000001";

        let chunk_v1 = make_chunk(id, doc_id, ws_id, vec![1.0, 0.0, 0.0]);
        store.upsert(&[chunk_v1]).await.unwrap();

        // Upsert same chunk_id with different embedding
        let chunk_v2 = make_chunk(id, doc_id, ws_id, vec![0.0, 1.0, 0.0]);
        store.upsert(&[chunk_v2]).await.unwrap();

        let query = SearchQuery {
            text: "test".to_string(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let results = store.search(&[0.0, 1.0, 0.0], &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!((results[0].score - 1.0).abs() < 1e-6); // matches new embedding
    }
}
