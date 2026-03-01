use std::time::Duration;

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
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
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
        // Separate system message from conversation messages.
        // Claude API expects system as a top-level parameter, not in the messages array.
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
        });

        if let Some(system) = system_text {
            body["system"] = serde_json::Value::String(system);
        }

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

        // Claude API returns content as an array of content blocks.
        // Extract text from all text blocks.
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

        Ok(content)
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
