use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;

pub struct OpenAiEmbeddingProvider {
    _api_key: String,
    _model: String,
    dimension: usize,
}

impl OpenAiEmbeddingProvider {
    pub fn new(api_key: &str, model: &str, dimension: usize) -> Self {
        Self {
            _api_key: api_key.to_string(),
            _model: model.to_string(),
            dimension,
        }
    }
}

#[async_trait]
impl EmbeddingModel for OpenAiEmbeddingProvider {
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        todo!("OpenAI Embedding API integration not yet implemented")
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
