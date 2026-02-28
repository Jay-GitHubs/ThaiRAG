use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use thairag_core::ThaiRagError;

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn generate(&self, messages: &[ChatMessage], _max_tokens: Option<u32>) -> Result<String> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(e.to_string()))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(e.to_string()))?;

        json["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing content in Ollama response".into()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
