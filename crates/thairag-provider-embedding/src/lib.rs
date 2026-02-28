pub mod fastembed_provider;
pub mod openai_embedding;

use thairag_config::schema::EmbeddingConfig;
use thairag_core::traits::EmbeddingModel;
use thairag_core::types::EmbeddingKind;

pub fn create_embedding_provider(config: &EmbeddingConfig) -> Box<dyn EmbeddingModel> {
    match config.kind {
        EmbeddingKind::Fastembed => {
            Box::new(fastembed_provider::FastEmbedProvider::new(&config.model, config.dimension))
        }
        EmbeddingKind::OpenAi => Box::new(openai_embedding::OpenAiEmbeddingProvider::new(
            &config.api_key,
            &config.model,
            config.dimension,
        )),
    }
}
