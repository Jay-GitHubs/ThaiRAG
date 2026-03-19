pub mod cohere_embedding;
pub mod fastembed_provider;
pub mod ollama_embedding;
pub mod openai_embedding;

use thairag_config::schema::EmbeddingConfig;
use thairag_core::traits::EmbeddingModel;
use thairag_core::types::EmbeddingKind;

pub fn create_embedding_provider(config: &EmbeddingConfig) -> Box<dyn EmbeddingModel> {
    create_embedding_provider_with_options(config, None)
}

pub fn create_embedding_provider_with_options(
    config: &EmbeddingConfig,
    ollama_keep_alive: Option<&str>,
) -> Box<dyn EmbeddingModel> {
    match config.kind {
        EmbeddingKind::Fastembed => Box::new(fastembed_provider::FastEmbedProvider::new(
            &config.model,
            config.dimension,
        )),
        EmbeddingKind::OpenAi => Box::new(openai_embedding::OpenAiEmbeddingProvider::new(
            &config.api_key,
            &config.model,
            config.dimension,
            &config.base_url,
        )),
        EmbeddingKind::Ollama => {
            Box::new(ollama_embedding::OllamaEmbeddingProvider::with_keep_alive(
                &config.base_url,
                &config.model,
                config.dimension,
                ollama_keep_alive,
            ))
        }
        EmbeddingKind::Cohere => Box::new(cohere_embedding::CohereEmbeddingProvider::new(
            &config.api_key,
            &config.model,
            config.dimension,
        )),
    }
}
