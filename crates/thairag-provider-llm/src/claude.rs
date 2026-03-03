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

pub struct ClaudeProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl ClaudeProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, "Initialized Claude provider");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    fn build_request_body(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
        stream: bool,
    ) -> serde_json::Value {
        let system_text: Option<String> = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .reduce(|mut acc, s| {
                acc.push('\n');
                acc.push_str(&s);
                acc
            });

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "max_tokens": max_tokens.unwrap_or(4096),
            "stream": stream,
        });

        if let Some(system) = system_text {
            body["system"] = serde_json::Value::String(system);
        }

        body
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<LlmResponse> {
        let body = self.build_request_body(messages, max_tokens, false);

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Claude request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Claude returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Failed to parse Claude response: {e}")))?;

        let content = json["content"]
            .as_array()
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing content array in Claude response".into()))?
            .iter()
            .filter(|block| block["type"].as_str() == Some("text"))
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("");

        if content.is_empty() {
            return Err(ThaiRagError::LlmProvider("No text content in Claude response".into()));
        }

        let usage = LlmUsage {
            prompt_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }

    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate_stream(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let body = self.build_request_body(messages, max_tokens, true);

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Claude stream request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Claude returned HTTP {status}: {error_body}"
            )));
        }

        use tokio_stream::StreamExt;
        let mut byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buf = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk
                    .map_err(|e| ThaiRagError::LlmProvider(format!("Claude stream read error: {e}")))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                // Claude SSE: "event: <type>\ndata: <json>\n\n"
                while let Some(double_newline) = buf.find("\n\n") {
                    let event_block = buf[..double_newline].to_string();
                    buf = buf[double_newline + 2..].to_string();

                    // Extract data line
                    let data = event_block
                        .lines()
                        .find(|l| l.starts_with("data: "))
                        .map(|l| &l[6..]);

                    let Some(data) = data else { continue };

                    let json: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let event_type = json["type"].as_str().unwrap_or("");
                    match event_type {
                        "content_block_delta" => {
                            if let Some(text) = json["delta"]["text"].as_str() {
                                if !text.is_empty() {
                                    yield text.to_string();
                                }
                            }
                        }
                        "message_stop" => return,
                        _ => {}
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
