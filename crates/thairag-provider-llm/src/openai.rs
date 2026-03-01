use std::pin::Pin;
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::Stream;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use thairag_core::ThaiRagError;
use tracing::{info, instrument};

pub struct OpenAiLlmProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenAiLlmProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, "Initialized OpenAI LLM provider");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiLlmProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("OpenAI request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "OpenAI returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Failed to parse OpenAI response: {e}")))?;

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing content in OpenAI response".into()))
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

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("OpenAI stream request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "OpenAI returned HTTP {status}: {error_body}"
            )));
        }

        use tokio_stream::StreamExt;
        let mut byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buf = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk
                    .map_err(|e| ThaiRagError::LlmProvider(format!("OpenAI stream read error: {e}")))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                // OpenAI SSE: "data: <json>\n\n" or "data: [DONE]\n\n"
                while let Some(double_newline) = buf.find("\n\n") {
                    let line = buf[..double_newline].trim().to_string();
                    buf = buf[double_newline + 2..].to_string();

                    let Some(data) = line.strip_prefix("data: ") else { continue };

                    if data == "[DONE]" {
                        return;
                    }

                    let json: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
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
