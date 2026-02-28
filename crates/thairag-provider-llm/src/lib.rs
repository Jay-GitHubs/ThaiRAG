pub mod claude;
pub mod ollama;
pub mod openai;

use thairag_config::schema::LlmConfig;
use thairag_core::traits::LlmProvider;
use thairag_core::types::LlmKind;

pub fn create_llm_provider(config: &LlmConfig) -> Box<dyn LlmProvider> {
    match config.kind {
        LlmKind::Ollama => Box::new(ollama::OllamaProvider::new(&config.base_url, &config.model)),
        LlmKind::Claude => Box::new(claude::ClaudeProvider::new(&config.api_key, &config.model)),
        LlmKind::OpenAi => Box::new(openai::OpenAiLlmProvider::new(&config.api_key, &config.model)),
    }
}
