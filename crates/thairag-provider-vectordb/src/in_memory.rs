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
            .filter(|chunk| {
                query.workspace_ids.is_empty()
                    || query.workspace_ids.contains(&chunk.workspace_id)
            })
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

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(query.top_k);
        Ok(results)
    }

    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        let mut store = self.chunks.write().unwrap();
        store.retain(|_, chunk| chunk.doc_id != doc_id);
        Ok(())
    }
}
