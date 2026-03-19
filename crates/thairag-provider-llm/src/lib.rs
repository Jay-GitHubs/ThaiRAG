pub mod claude;
pub mod gemini;
pub mod ollama;
pub mod openai;

use thairag_config::schema::LlmConfig;
use thairag_core::traits::LlmProvider;
use thairag_core::types::LlmKind;

pub fn create_llm_provider(config: &LlmConfig) -> Box<dyn LlmProvider> {
    create_llm_provider_with_timeout(config, 120)
}

pub fn create_llm_provider_with_timeout(
    config: &LlmConfig,
    timeout_secs: u64,
) -> Box<dyn LlmProvider> {
    create_llm_provider_with_options(config, timeout_secs, None)
}

pub fn create_llm_provider_with_options(
    config: &LlmConfig,
    timeout_secs: u64,
    ollama_keep_alive: Option<&str>,
) -> Box<dyn LlmProvider> {
    match config.kind {
        LlmKind::Ollama => Box::new(ollama::OllamaProvider::with_timeout_and_keep_alive(
            &config.base_url,
            &config.model,
            timeout_secs,
            ollama_keep_alive,
        )),
        LlmKind::Claude => Box::new(claude::ClaudeProvider::with_timeout(
            &config.api_key,
            &config.model,
            timeout_secs,
        )),
        LlmKind::OpenAi | LlmKind::OpenAiCompatible => {
            Box::new(openai::OpenAiLlmProvider::with_timeout(
                &config.api_key,
                &config.model,
                &config.base_url,
                timeout_secs,
            ))
        }
        LlmKind::Gemini => Box::new(gemini::GeminiProvider::with_timeout(
            &config.api_key,
            &config.model,
            timeout_secs,
        )),
    }
}
