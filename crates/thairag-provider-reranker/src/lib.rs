pub mod cohere;
pub mod jina;
pub mod passthrough;

use thairag_config::schema::RerankerConfig;
use thairag_core::traits::Reranker;
use thairag_core::types::RerankerKind;

pub fn create_reranker(config: &RerankerConfig) -> Box<dyn Reranker> {
    match config.kind {
        RerankerKind::Passthrough => Box::new(passthrough::PassthroughReranker::new()),
        RerankerKind::Cohere => {
            Box::new(cohere::CohereReranker::new(&config.api_key, &config.model))
        }
        RerankerKind::Jina => Box::new(jina::JinaReranker::new(&config.api_key, &config.model)),
    }
}
