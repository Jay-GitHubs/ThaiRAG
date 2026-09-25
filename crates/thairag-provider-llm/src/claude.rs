use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, VisionMessage};
use tracing::{info, instrument};

pub struct ClaudeProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    temperature: Option<f32>,
    sampling: thairag_core::types::SamplingParams,
    reasoning: thairag_core::types::ReasoningParams,
}

impl ClaudeProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_timeout(api_key, model, 120)
    }

    pub fn with_timeout(api_key: &str, model: &str, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, timeout_secs, "Initialized Claude provider");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            temperature: None,
            sampling: Default::default(),
            reasoning: Default::default(),
        }
    }

    pub fn with_sampling(
        mut self,
        temperature: Option<f32>,
        sampling: thairag_core::types::SamplingParams,
    ) -> Self {
        self.temperature = temperature;
        self.sampling = sampling;
        self
    }

    pub fn with_reasoning(mut self, reasoning: thairag_core::types::ReasoningParams) -> Self {
        self.reasoning = reasoning;
        self
    }

    /// Extended thinking: `thinking: {type: enabled, budget_tokens}` when the
    /// toggle is on. The API requires `budget_tokens` ≥ 1024 and
    /// `max_tokens` > budget, and forbids temperature / top_p / top_k
    /// overrides while thinking — so those are dropped (logged). Thinking
    /// blocks in the response are already filtered out by the parsers.
    fn apply_reasoning(&self, body: &mut serde_json::Value) {
        if self.reasoning.thinking != Some(true) {
            return;
        }
        let budget = self
            .reasoning
            .thinking_budget_tokens
            .unwrap_or(4096)
            .max(1024);
        body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": budget});
        let max = body["max_tokens"].as_u64().unwrap_or(4096) as u32;
        if max <= budget {
            body["max_tokens"] = serde_json::json!(budget + 4096);
        }
        if let Some(obj) = body.as_object_mut() {
            let dropped: Vec<&str> = ["temperature", "top_p", "top_k"]
                .into_iter()
                .filter(|k| obj.remove(*k).is_some())
                .collect();
            if !dropped.is_empty() {
                tracing::warn!(
                    ?dropped,
                    "Claude extended thinking forbids sampling overrides — dropped"
                );
            }
        }
    }

    /// Messages API sampling: `temperature` and `top_p` are mutually
    /// exclusive on current Claude models, so temperature wins when both are
    /// set; `top_k` and `stop_sequences` map directly; seed / penalties are
    /// not supported and never sent.
    fn apply_sampling(&self, body: &mut serde_json::Value) {
        let s = &self.sampling;
        if let Some(t) = self.temperature {
            body["temperature"] = serde_json::json!(thairag_core::types::f32_as_json_number(t));
        } else if let Some(v) = s.top_p {
            body["top_p"] = serde_json::json!(v);
        }
        if let Some(v) = s.top_k {
            body["top_k"] = serde_json::json!(v);
        }
        if !s.stop.is_empty() {
            body["stop_sequences"] = serde_json::json!(s.stop);
        }
        self.apply_reasoning(body);
        s.merge_extra_into(body);
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
        self.apply_sampling(&mut body);

        body
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
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

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse Claude response: {e}"))
        })?;

        let content = json["content"]
            .as_array()
            .ok_or_else(|| {
                ThaiRagError::LlmProvider("Missing content array in Claude response".into())
            })?
            .iter()
            .filter(|block| block["type"].as_str() == Some("text"))
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("");

        if content.is_empty() {
            return Err(ThaiRagError::LlmProvider(
                "No text content in Claude response".into(),
            ));
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
    ) -> Result<LlmStreamResponse> {
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

        let usage_cell: Arc<Mutex<Option<LlmUsage>>> = Arc::new(Mutex::new(None));
        let usage_writer = Arc::clone(&usage_cell);

        use tokio_stream::StreamExt;
        let mut byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buf = String::new();
            let mut input_tokens: u32 = 0;
            let mut output_tokens: u32 = 0;
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
                        "message_start" => {
                            input_tokens = json["message"]["usage"]["input_tokens"]
                                .as_u64()
                                .unwrap_or(0) as u32;
                        }
                        "content_block_delta" => {
                            if let Some(text) = json["delta"]["text"].as_str()
                                && !text.is_empty()
                            {
                                yield text.to_string();
                            }
                        }
                        "message_delta" => {
                            output_tokens = json["usage"]["output_tokens"]
                                .as_u64()
                                .unwrap_or(0) as u32;
                        }
                        "message_stop" => {
                            *usage_writer.lock().unwrap() = Some(LlmUsage {
                                prompt_tokens: input_tokens,
                                completion_tokens: output_tokens,
                            });
                            return;
                        }
                        _ => {}
                    }
                }
            }
            // Stream ended without message_stop — write what we have
            *usage_writer.lock().unwrap() = Some(LlmUsage {
                prompt_tokens: input_tokens,
                completion_tokens: output_tokens,
            });
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
        // All Claude 3+ models support vision
        let m = &self.model;
        m.contains("claude-3")
            || m.contains("claude-opus-4")
            || m.contains("claude-sonnet-4")
            || m.contains("claude-haiku-4")
    }

    async fn generate_vision(
        &self,
        messages: &[VisionMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let system_text: Option<String> = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.text.clone())
            .reduce(|mut acc, s| {
                acc.push('\n');
                acc.push_str(&s);
                acc
            });

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();
                // Add images first
                for img in &m.images {
                    content_blocks.push(serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": img.media_type,
                            "data": img.base64_data,
                        }
                    }));
                }
                // Add text
                if !m.text.is_empty() {
                    content_blocks.push(serde_json::json!({
                        "type": "text",
                        "text": m.text,
                    }));
                }
                serde_json::json!({
                    "role": m.role,
                    "content": content_blocks,
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
        self.apply_sampling(&mut body);

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Claude vision request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Claude returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse Claude response: {e}"))
        })?;

        let content = json["content"]
            .as_array()
            .ok_or_else(|| {
                ThaiRagError::LlmProvider("Missing content array in Claude response".into())
            })?
            .iter()
            .filter(|block| block["type"].as_str() == Some("text"))
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("");

        if content.is_empty() {
            return Err(ThaiRagError::LlmProvider(
                "No text content in Claude vision response".into(),
            ));
        }

        let usage = LlmUsage {
            prompt_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::SamplingParams;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            images: vec![],
        }
    }

    #[test]
    fn extended_thinking_sets_budget_bumps_max_tokens_and_drops_sampling() {
        use thairag_core::types::ReasoningParams;
        let p = ClaudeProvider::new("k", "claude-x")
            .with_sampling(
                Some(0.2),
                SamplingParams {
                    top_k: Some(5),
                    ..Default::default()
                },
            )
            .with_reasoning(ReasoningParams {
                thinking: Some(true),
                thinking_budget_tokens: Some(8000),
                ..Default::default()
            });
        let body = p.build_request_body(&[msg("user", "hi")], Some(4096), false);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 8000);
        assert!(
            body["max_tokens"].as_u64().unwrap() > 8000,
            "max_tokens raised above the budget"
        );
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_k").is_none());
        // Off / default → no thinking block, sampling untouched.
        let p = ClaudeProvider::new("k", "claude-x").with_sampling(Some(0.2), Default::default());
        let body = p.build_request_body(&[msg("user", "hi")], None, false);
        assert!(body.get("thinking").is_none());
        assert!(body.get("temperature").is_some());
    }

    #[test]
    fn temperature_wins_over_top_p_and_stop_maps_to_stop_sequences() {
        let sampling = SamplingParams {
            top_p: Some(0.9),
            top_k: Some(20),
            seed: Some(7),
            stop: vec!["END".into()],
            ..Default::default()
        };
        let p = ClaudeProvider::new("k", "claude-x").with_sampling(Some(0.2), sampling.clone());
        let body = p.build_request_body(&[msg("user", "hi")], None, false);
        assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
        assert!(body.get("top_p").is_none(), "never both");
        assert_eq!(body["top_k"], 20);
        assert_eq!(body["stop_sequences"], serde_json::json!(["END"]));
        assert!(
            body.get("seed").is_none(),
            "unsupported by the Messages API"
        );

        let p = ClaudeProvider::new("k", "claude-x").with_sampling(None, sampling);
        let body = p.build_request_body(&[msg("user", "hi")], None, false);
        assert!((body["top_p"].as_f64().unwrap() - 0.9).abs() < 1e-6);
        assert!(body.get("temperature").is_none());
    }
}
