use std::pin::Pin;
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::Stream;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmUsage};
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
    async fn generate(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<LlmResponse> {
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

        let content = json["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing content in Ollama response".into()))?;

        let usage = LlmUsage {
            prompt_tokens: json["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["eval_count"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }

    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate_stream(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
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
            .map_err(|e| ThaiRagError::LlmProvider(format!("Ollama stream request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Ollama returned HTTP {status}: {error_body}"
            )));
        }

        use tokio_stream::StreamExt;
        let mut byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buf = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk
                    .map_err(|e| ThaiRagError::LlmProvider(format!("Ollama stream read error: {e}")))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                // Ollama sends NDJSON — one JSON object per line
                while let Some(newline_pos) = buf.find('\n') {
                    let line = buf[..newline_pos].trim().to_string();
                    buf = buf[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    let json: serde_json::Value = serde_json::from_str(&line)
                        .map_err(|e| ThaiRagError::LlmProvider(format!("Ollama JSON parse error: {e}")))?;

                    if json["done"].as_bool() == Some(true) {
                        return;
                    }

                    if let Some(content) = json["message"]["content"].as_str() {
                        if !content.is_empty() {
                            yield content.to_string();
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
