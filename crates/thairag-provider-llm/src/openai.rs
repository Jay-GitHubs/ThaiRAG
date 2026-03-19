use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, VisionMessage};
use tracing::{info, instrument};

pub struct OpenAiLlmProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiLlmProvider {
    pub fn new(api_key: &str, model: &str, base_url: &str) -> Self {
        Self::with_timeout(api_key, model, base_url, 120)
    }

    pub fn with_timeout(api_key: &str, model: &str, base_url: &str, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build reqwest client");

        let base_url = if base_url.is_empty() {
            "https://api.openai.com".to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };

        info!(
            model,
            base_url, timeout_secs, "Initialized OpenAI LLM provider"
        );

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiLlmProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
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

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse OpenAI response: {e}"))
        })?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                ThaiRagError::LlmProvider("Missing content in OpenAI response".into())
            })?;

        let usage = LlmUsage {
            prompt_tokens: json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }

    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate_stream(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmStreamResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
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

        let usage_cell: Arc<Mutex<Option<LlmUsage>>> = Arc::new(Mutex::new(None));
        let usage_writer = Arc::clone(&usage_cell);

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

                    // Usage-only chunk: empty choices + usage present
                    if let Some(usage) = json.get("usage").filter(|u| !u.is_null()) {
                        let choices = json["choices"].as_array();
                        if choices.is_none_or(|c| c.is_empty()) {
                            *usage_writer.lock().unwrap() = Some(LlmUsage {
                                prompt_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                                completion_tokens: usage["completion_tokens"].as_u64().unwrap_or(0) as u32,
                            });
                            continue;
                        }
                    }

                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str()
                        && !content.is_empty()
                    {
                        yield content.to_string();
                    }
                }
            }
        };

        Ok(LlmStreamResponse {
            stream: Box::pin(stream),
            usage: usage_cell,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn supports_vision(&self) -> bool {
        let m = &self.model;
        m.contains("gpt-4o")
            || m.contains("gpt-4.1")
            || m.contains("gpt-4-vision")
            || m.starts_with("o3")
            || m.starts_with("o4")
    }

    async fn generate_vision(
        &self,
        messages: &[VisionMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let mut content: Vec<serde_json::Value> = Vec::new();
                // Add images
                for img in &m.images {
                    let data_url = format!("data:{};base64,{}", img.media_type, img.base64_data);
                    content.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": data_url },
                    }));
                }
                // Add text
                if !m.text.is_empty() {
                    content.push(serde_json::json!({
                        "type": "text",
                        "text": m.text,
                    }));
                }
                serde_json::json!({
                    "role": m.role,
                    "content": content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("OpenAI vision request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "OpenAI returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse OpenAI response: {e}"))
        })?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                ThaiRagError::LlmProvider("Missing content in OpenAI response".into())
            })?;

        let usage = LlmUsage {
            prompt_tokens: json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }
}
