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
    /// Explicit vision-capability override. `None` falls back to the
    /// model-name heuristic in `supports_vision()`.
    vision_override: Option<bool>,
}

impl OpenAiLlmProvider {
    pub fn new(api_key: &str, model: &str, base_url: &str) -> Self {
        Self::with_timeout(api_key, model, base_url, 120)
    }

    pub fn with_timeout(api_key: &str, model: &str, base_url: &str, timeout_secs: u64) -> Self {
        Self::with_options(api_key, model, base_url, timeout_secs, None)
    }

    pub fn with_options(
        api_key: &str,
        model: &str,
        base_url: &str,
        timeout_secs: u64,
        vision_override: Option<bool>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build reqwest client");

        let base_url = if base_url.is_empty() {
            "https://api.openai.com".to_string()
        } else {
            // Store the base without a trailing `/v1`; request sites append
            // `/v1/...` themselves. This accepts both `https://host` and
            // `https://host/v1/` without producing a duplicated `/v1/v1/...`.
            let trimmed = base_url.trim_end_matches('/');
            trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
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
            vision_override,
        }
    }
}

/// Move every `system` message to the front, merged into one.
///
/// Qwen/vLLM chat templates (and other strict OpenAI-compatible backends)
/// accept exactly one `system` message and only at index 0 — anything else is
/// rejected with `400 "System message must be at the beginning."`. The chat
/// pipeline appends late `system` messages (attachment documents, image
/// KB context) after the conversation history, which OpenAI proper tolerates
/// but the gateway does not. Normalising here keeps the pipeline
/// provider-agnostic and is harmless for OpenAI itself.
///
/// The relative order of non-system messages is unchanged; system contents
/// are joined in their original order with a blank line between them.
fn hoist_system_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let system_text: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == "system" && !m.content.is_empty())
        .map(|m| m.content.as_str())
        .collect();
    let has_system = messages.iter().any(|m| m.role == "system");
    let mut out = Vec::with_capacity(messages.len());
    if has_system {
        out.push(ChatMessage {
            role: "system".into(),
            content: system_text.join("\n\n"),
            images: vec![],
        });
    }
    // Text endpoints (`generate`, `generate_stream`, `generate_structured`)
    // never receive image parts: `ChatMessage.images` would otherwise be
    // serialised as an unknown `images` field the API rejects. Vision goes
    // through `generate_vision`.
    out.extend(
        messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                images: vec![],
            }),
    );
    out
}

/// `VisionMessage` sibling of [`hoist_system_messages`].
fn hoist_system_vision_messages(messages: &[VisionMessage]) -> Vec<VisionMessage> {
    let mut system_text: Vec<&str> = Vec::new();
    let mut system_images = Vec::new();
    for m in messages.iter().filter(|m| m.role == "system") {
        if !m.text.is_empty() {
            system_text.push(m.text.as_str());
        }
        system_images.extend(m.images.iter().cloned());
    }
    let has_system = messages.iter().any(|m| m.role == "system");
    let mut out = Vec::with_capacity(messages.len());
    if has_system {
        out.push(VisionMessage {
            role: "system".into(),
            text: system_text.join("\n\n"),
            images: system_images,
        });
    }
    out.extend(messages.iter().filter(|m| m.role != "system").cloned());
    out
}

/// Serialise vision messages into OpenAI `content` arrays (image parts as
/// base64 data URLs, then the text part), after hoisting system messages.
fn vision_messages_to_json(messages: &[VisionMessage]) -> Vec<serde_json::Value> {
    hoist_system_vision_messages(messages)
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
        .collect()
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
            "messages": hoist_system_messages(messages),
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = crate::retry::send_with_retry(
            || {
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
            },
            "openai.generate",
        )
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

    /// Schema-enforced generation via OpenAI `response_format: json_schema`
    /// (supported by vLLM and most OpenAI-compatible gateways). Without this
    /// override the trait DEFAULT silently ignores the schema and every
    /// "schema-enforced" agent (tree builder, chunk enricher, context curator)
    /// runs prompt-only JSON on gateway deployments — observed live: PageIndex
    /// tree builds returning unparseable/empty structures. Gateways that
    /// reject `response_format` (HTTP 4xx) fall back to plain generation once,
    /// loudly, so behavior degrades to the status quo rather than failing.
    #[instrument(skip(self, messages, json_schema), fields(model = %self.model))]
    async fn generate_structured(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
        json_schema: &serde_json::Value,
    ) -> Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": hoist_system_messages(messages),
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_output",
                    "strict": true,
                    "schema": json_schema,
                },
            },
        });
        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = crate::retry::send_with_retry(
            || {
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
            },
            "openai.generate_structured",
        )
        .await
        .map_err(|e| ThaiRagError::LlmProvider(format!("OpenAI request failed: {e}")))?;

        let status = resp.status();
        if status.is_client_error() {
            // Gateway doesn't support response_format — degrade to prompt-only
            // JSON (the pre-override behavior), but say so.
            let error_body = resp.text().await.unwrap_or_default();
            tracing::warn!(
                %status,
                error = %error_body.chars().take(200).collect::<String>(),
                "gateway rejected response_format json_schema — falling back to prompt-only JSON"
            );
            return self.generate(messages, max_tokens).await;
        }
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
            "messages": hoist_system_messages(messages),
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        // Retry only the initial request/connection — once we start consuming
        // the byte stream a retry would duplicate partial output.
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = crate::retry::send_with_retry(
            || {
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
            },
            "openai.generate_stream",
        )
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
        // Explicit config override wins (e.g. an OpenAI-compatible gateway's
        // `qwen2.5-vl-7b`, which the name heuristic below wouldn't recognize).
        if let Some(v) = self.vision_override {
            return v;
        }
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
        let api_messages = vision_messages_to_json(messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
        });

        if let Some(max) = max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = crate::retry::send_with_retry(
            || {
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
            },
            "openai.generate_vision",
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_normalization_strips_trailing_v1_and_slash() {
        // A gateway documented as `https://host/v1/` must not become
        // `https://host/v1/v1/chat/completions`.
        let p = OpenAiLlmProvider::new("k", "m", "https://llm.jay-tech-ai.com/v1/");
        assert_eq!(p.base_url, "https://llm.jay-tech-ai.com");

        // Without a trailing slash.
        let p = OpenAiLlmProvider::new("k", "m", "https://llm.jay-tech-ai.com/v1");
        assert_eq!(p.base_url, "https://llm.jay-tech-ai.com");

        // No `/v1` suffix — left as-is (request sites append `/v1/...`).
        let p = OpenAiLlmProvider::new("k", "m", "https://api.groq.com/openai");
        assert_eq!(p.base_url, "https://api.groq.com/openai");

        // Trailing slash only.
        let p = OpenAiLlmProvider::new("k", "m", "https://host/");
        assert_eq!(p.base_url, "https://host");

        // Empty → default OpenAI host.
        let p = OpenAiLlmProvider::new("k", "m", "");
        assert_eq!(p.base_url, "https://api.openai.com");
    }

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            images: vec![],
        }
    }

    fn roles(msgs: &[ChatMessage]) -> Vec<&str> {
        msgs.iter().map(|m| m.role.as_str()).collect()
    }

    #[test]
    fn hoist_moves_mid_conversation_system_to_front() {
        // Shape produced by the chat pipeline for an attached document: the
        // main system prompt, history, then a late `[Document: …]` system.
        let input = vec![
            msg("system", "You are ThaiRAG."),
            msg("user", "hi"),
            msg("assistant", "hello"),
            msg("system", "[Document: loan.pdf]\nrate 3.5%"),
            msg("user", "what is the rate?"),
        ];
        let out = hoist_system_messages(&input);
        assert_eq!(roles(&out), ["system", "user", "assistant", "user"]);
        assert!(out[0].content.contains("[Document: loan.pdf]"));
        assert_eq!(out[1].content, "hi");
        assert_eq!(out[2].content, "hello");
        assert_eq!(out[3].content, "what is the rate?");
    }

    #[test]
    fn hoist_merges_multiple_system_messages_in_order() {
        let input = vec![
            msg("user", "u1"),
            msg("system", "first"),
            msg("user", "u2"),
            msg("system", "second"),
        ];
        let out = hoist_system_messages(&input);
        assert_eq!(out.iter().filter(|m| m.role == "system").count(), 1);
        assert_eq!(out[0].content, "first\n\nsecond");
        assert_eq!(roles(&out), ["system", "user", "user"]);
    }

    #[test]
    fn hoist_is_identity_without_system_messages() {
        let input = vec![msg("user", "u1"), msg("assistant", "a1"), msg("user", "u2")];
        let out = hoist_system_messages(&input);
        assert_eq!(roles(&out), roles(&input));
        for (a, b) in out.iter().zip(&input) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn hoist_drops_image_parts_for_text_endpoints() {
        let mut m = msg("user", "look");
        m.images.push(thairag_core::types::ImageContent {
            base64_data: "AQID".into(),
            media_type: "image/png".into(),
        });
        let out = hoist_system_messages(&[msg("system", "sys"), m]);
        assert!(out.iter().all(|m| m.images.is_empty()));
        assert_eq!(out[1].content, "look");
    }

    #[test]
    fn hoist_keeps_leading_single_system_unchanged() {
        let input = vec![msg("system", "sys"), msg("user", "u1")];
        let out = hoist_system_messages(&input);
        assert_eq!(roles(&out), ["system", "user"]);
        assert_eq!(out[0].content, "sys");
    }

    #[test]
    fn vision_json_hoists_late_system_and_keeps_image_parts() {
        use thairag_core::types::ImageContent;
        let input = vec![
            VisionMessage {
                role: "system".into(),
                text: "You are ThaiRAG.".into(),
                images: vec![],
            },
            VisionMessage {
                role: "user".into(),
                text: "what is in this picture?".into(),
                images: vec![ImageContent {
                    base64_data: "AAAA".into(),
                    media_type: "image/png".into(),
                }],
            },
            VisionMessage {
                role: "system".into(),
                text: "Similar KB images: chart.png".into(),
                images: vec![],
            },
        ];
        let out = vision_messages_to_json(&input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "system");
        let sys_text = out[0]["content"][0]["text"].as_str().unwrap();
        assert!(sys_text.contains("You are ThaiRAG."));
        assert!(sys_text.contains("Similar KB images: chart.png"));

        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[1]["content"][0]["type"], "image_url");
        assert_eq!(
            out[1]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
        assert_eq!(out[1]["content"][1]["text"], "what is in this picture?");
    }
}
