use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;

pub struct ClaudeProvider {
    _api_key: String,
    model: String,
}

impl ClaudeProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            _api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn generate(&self, _messages: &[ChatMessage], _max_tokens: Option<u32>) -> Result<String> {
        todo!("Claude API integration not yet implemented")
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
