use std::time::Duration;

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use thairag_core::ThaiRagError;
use tracing::{info, instrument};

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");

        info!(base_url, model, "Initialized Ollama provider");

        Self {
            client,
            base_url: base_url.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

        if let Some(num_predict) = max_tokens {
            body["options"] = serde_json::json!({ "num_predict": num_predict });
        }

        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Ollama request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Ollama returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Failed to parse Ollama response: {e}")))?;

        json["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing content in Ollama response".into()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
