use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::Reranker;
use thairag_core::types::SearchResult;

/// Passthrough reranker: returns results as-is without reranking.
/// Used in free tier or when no external reranker is configured.
pub struct PassthroughReranker;

impl PassthroughReranker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PassthroughReranker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Reranker for PassthroughReranker {
    async fn rerank(&self, _query: &str, results: Vec<SearchResult>) -> Result<Vec<SearchResult>> {
        Ok(results)
    }
}
