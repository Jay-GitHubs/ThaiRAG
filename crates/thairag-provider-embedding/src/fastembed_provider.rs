use std::sync::Arc;

use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use tracing::info;

pub struct FastEmbedProvider {
    model: Arc<TextEmbedding>,
    dimension: usize,
}

impl FastEmbedProvider {
    pub fn new(model_name: &str, dimension: usize) -> Self {
        let variant = match model_name {
            "BAAI/bge-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
            "BAAI/bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
            "BAAI/bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
            "sentence-transformers/all-MiniLM-L6-v2" => EmbeddingModel::AllMiniLML6V2,
            "sentence-transformers/all-MiniLM-L12-v2" => EmbeddingModel::AllMiniLML12V2,
            _ => {
                info!(
                    model_name,
                    "Unknown model name, falling back to BGESmallENV15"
                );
                EmbeddingModel::BGESmallENV15
            }
        };

        info!(
            ?variant,
            "Initializing FastEmbed model (this may download on first run)"
        );
        let model = TextEmbedding::try_new(InitOptions::new(variant))
            .expect("Failed to initialize FastEmbed model");
        info!("FastEmbed model initialized successfully");

        Self {
            model: Arc::new(model),
            dimension,
        }
    }
}

#[async_trait]
impl thairag_core::traits::EmbeddingModel for FastEmbedProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let model = Arc::clone(&self.model);
        let texts: Vec<String> = texts.to_vec();

        tokio::task::spawn_blocking(move || {
            model
                .embed(texts, None)
                .map_err(|e| ThaiRagError::Embedding(e.to_string()))
        })
        .await
        .map_err(|e| ThaiRagError::Embedding(format!("spawn_blocking join error: {e}")))?
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
