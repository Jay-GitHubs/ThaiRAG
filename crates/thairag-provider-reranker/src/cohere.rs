use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::Reranker;
use thairag_core::types::SearchResult;

pub struct CohereReranker {
    _api_key: String,
    _model: String,
}

impl CohereReranker {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            _api_key: api_key.to_string(),
            _model: model.to_string(),
        }
    }
}

#[async_trait]
impl Reranker for CohereReranker {
    async fn rerank(&self, _query: &str, _results: Vec<SearchResult>) -> Result<Vec<SearchResult>> {
        todo!("Cohere reranker API integration not yet implemented")
    }
}
