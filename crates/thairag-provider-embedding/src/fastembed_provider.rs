use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;

pub struct FastEmbedProvider {
    dimension: usize,
}

impl FastEmbedProvider {
    pub fn new(_model_name: &str, dimension: usize) -> Self {
        Self { dimension }
    }
}

#[async_trait]
impl EmbeddingModel for FastEmbedProvider {
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        todo!("FastEmbed integration not yet implemented")
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
