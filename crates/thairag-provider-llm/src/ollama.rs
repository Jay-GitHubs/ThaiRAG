use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, VisionMessage};
use tracing::{info, instrument};

/// Heuristic: does this Ollama model id denote a vision-capable model?
///
/// Ollama doesn't expose model capabilities over its API, so we match on the
/// model id. Kept broad and future-proof (the previous hardcoded list missed
/// valid models like `qwen3-vl`): any `*-vision` tag, the whole Qwen-VL family
/// (any generation), plus the known multimodal families. Shared by the
/// provider's `supports_vision()` and the settings capability check so the two
/// never drift.
pub fn is_ollama_vision_model(model: &str) -> bool {
    let m = model.to_lowercase();
    // Any "*-vision" tag: llama3.2-vision, granite3.2-vision, …
    m.contains("-vision")
        // Qwen-VL, any generation: qwen2-vl, qwen2.5vl, qwen3-vl, qwenvl, …
        || (m.contains("qwen") && m.contains("vl"))
        // Known multimodal families.
        || m.contains("llava")
        || m.contains("bakllava")
        || m.contains("minicpm-v")
        || m.contains("moondream")
        || m.contains("cogvlm")
        || m.contains("internvl")
        || m.contains("gemma3")
        || m.contains("mistral-small3")
        || m.contains("llama4")
}

/// Floor for the adaptive context window. Below this, model quality degrades and
/// the memory saved is negligible.
const MIN_NUM_CTX: usize = 2048;

pub struct OllamaProvider {
    client: reqwest::Client,
    /// Separate client for streaming — no overall timeout so long generations aren't killed.
    stream_client: reqwest::Client,
    base_url: String,
    model: String,
    keep_alive: Option<serde_json::Value>,
    /// Adaptive `num_ctx` ceiling. `0` = inherit the model's default context
    /// (don't send `num_ctx`).
    num_ctx_max: usize,
    /// Sampling temperature. `None` inherits the model's built-in default.
    temperature: Option<f32>,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self::with_timeout(base_url, model, 120)
    }

    pub fn with_timeout(base_url: &str, model: &str, timeout_secs: u64) -> Self {
        Self::with_timeout_and_keep_alive(base_url, model, timeout_secs, None)
    }

    pub fn with_timeout_and_keep_alive(
        base_url: &str,
        model: &str,
        timeout_secs: u64,
        keep_alive: Option<&str>,
    ) -> Self {
        Self::with_options(base_url, model, timeout_secs, keep_alive, 0, None)
    }

    pub fn with_options(
        base_url: &str,
        model: &str,
        timeout_secs: u64,
        keep_alive: Option<&str>,
        num_ctx_max: usize,
        temperature: Option<f32>,
    ) -> Self {
        // Non-streaming client: overall timeout covers the full request lifecycle
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build reqwest client");

        // Streaming client: no overall timeout so long generations aren't killed.
        // Only connect_timeout is set to detect unreachable servers quickly.
        let stream_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build reqwest streaming client");

        let keep_alive_val = keep_alive.map(|s| {
            // Try to parse as integer (e.g. "-1", "0"), otherwise keep as string (e.g. "5m")
            if let Ok(n) = s.parse::<i64>() {
                serde_json::Value::Number(serde_json::Number::from(n))
            } else {
                serde_json::Value::String(s.to_string())
            }
        });

        info!(
            base_url,
            model,
            timeout_secs,
            ?keep_alive,
            num_ctx_max,
            ?temperature,
            "Initialized Ollama provider"
        );

        Self {
            client,
            stream_client,
            base_url: base_url.to_string(),
            model: model.to_string(),
            keep_alive: keep_alive_val,
            num_ctx_max,
            temperature,
        }
    }

    /// Build the Ollama `options` object, attaching `num_predict` (output cap)
    /// and an adaptive `num_ctx` when enabled. Returns `None` when neither is
    /// set, so the request omits `options` entirely (preserves prior behavior).
    fn build_options(
        &self,
        max_tokens: Option<u32>,
        num_ctx: Option<usize>,
    ) -> Option<serde_json::Value> {
        let mut opts = serde_json::Map::new();
        if let Some(num_predict) = max_tokens {
            opts.insert("num_predict".into(), serde_json::json!(num_predict));
        }
        if let Some(ctx) = num_ctx {
            opts.insert("num_ctx".into(), serde_json::json!(ctx));
        }
        if let Some(temp) = self.temperature {
            opts.insert("temperature".into(), serde_json::json!(temp));
        }
        if opts.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(opts))
        }
    }

    /// Adaptive `num_ctx` for a text prompt: size to the estimated prompt tokens
    /// plus output headroom, bucketed to a power of two (so distinct prompt
    /// sizes don't each trigger an Ollama model reload), clamped to
    /// `[MIN_NUM_CTX, num_ctx_max]`. Returns `None` when the cap is disabled.
    fn text_num_ctx(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Option<usize> {
        if self.num_ctx_max == 0 {
            return None;
        }
        let prompt = estimate_message_tokens(messages);
        let needed = prompt.saturating_add(max_tokens.unwrap_or(0) as usize);
        Some(bucket_num_ctx(needed).clamp(MIN_NUM_CTX, self.num_ctx_max))
    }

    /// Adaptive `num_ctx` for a vision call. Image token counts depend on the
    /// model's dynamic-resolution tiling and aren't known up front, so request
    /// the full cap rather than risk truncating the image. Returns `None` when
    /// the cap is disabled.
    fn vision_num_ctx(&self) -> Option<usize> {
        if self.num_ctx_max == 0 {
            None
        } else {
            Some(self.num_ctx_max)
        }
    }
}

/// Estimate token count for a set of chat messages. Uses the common ~4-chars-
/// per-token heuristic over role + content, plus a small per-message overhead
/// for chat-template framing. Intentionally conservative (rounds up).
fn estimate_message_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|m| (m.role.len() + m.content.len()) / 4 + 8)
        .sum()
}

/// Round a token estimate up to the next power of two (floored at MIN_NUM_CTX).
/// Bucketing keeps the number of distinct `num_ctx` values small so Ollama
/// reuses a loaded model instead of reloading it for every prompt length.
fn bucket_num_ctx(tokens: usize) -> usize {
    tokens.max(MIN_NUM_CTX).next_power_of_two()
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

        if let Some(opts) = self.build_options(max_tokens, self.text_num_ctx(messages, max_tokens))
        {
            body["options"] = opts;
        }
        if let Some(ref ka) = self.keep_alive {
            body["keep_alive"] = ka.clone();
        }

        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format_ollama_error(&self.base_url, e)))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Ollama returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse Ollama response: {e}"))
        })?;

        let raw_content = json["message"]["content"].as_str().ok_or_else(|| {
            ThaiRagError::LlmProvider("Missing content in Ollama response".into())
        })?;

        // Strip <think>...</think> blocks (Qwen3, DeepSeek-R1, etc.)
        let content = thairag_core::strip_thinking_tags(raw_content);

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
    ) -> Result<LlmStreamResponse> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });

        if let Some(opts) = self.build_options(max_tokens, self.text_num_ctx(messages, max_tokens))
        {
            body["options"] = opts;
        }
        if let Some(ref ka) = self.keep_alive {
            body["keep_alive"] = ka.clone();
        }

        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .stream_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format_ollama_error(&self.base_url, e)))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Ollama returned HTTP {status}: {error_body}"
            )));
        }

        let usage_cell: Arc<Mutex<Option<LlmUsage>>> = Arc::new(Mutex::new(None));
        let usage_writer = Arc::clone(&usage_cell);

        use tokio_stream::StreamExt;
        let mut byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buf = String::new();
            let mut in_thinking = false; // Track <think> blocks in stream

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
                        *usage_writer.lock().unwrap() = Some(LlmUsage {
                            prompt_tokens: json["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
                            completion_tokens: json["eval_count"].as_u64().unwrap_or(0) as u32,
                        });
                        return;
                    }

                    if let Some(content) = json["message"]["content"].as_str()
                        && !content.is_empty()
                    {
                        // Suppress <think>...</think> tokens from stream
                        if content.contains("<think>") {
                            in_thinking = true;
                            continue;
                        }
                        if in_thinking {
                            if content.contains("</think>") {
                                in_thinking = false;
                                // Emit any text after </think> on the same token
                                if let Some(after) = content.split("</think>").nth(1) {
                                    let trimmed = after.trim();
                                    if !trimmed.is_empty() {
                                        yield trimmed.to_string();
                                    }
                                }
                            }
                            continue;
                        }
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
        is_ollama_vision_model(&self.model)
    }

    async fn generate_vision(
        &self,
        messages: &[VisionMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        // Ollama uses "images" field: array of base64 strings (no media type prefix)
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.text,
                });
                if !m.images.is_empty() {
                    let images: Vec<&str> = m
                        .images
                        .iter()
                        .map(|img| img.base64_data.as_str())
                        .collect();
                    msg["images"] = serde_json::json!(images);
                }
                msg
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "stream": false,
        });

        if let Some(opts) = self.build_options(max_tokens, self.vision_num_ctx()) {
            body["options"] = opts;
        }
        if let Some(ref ka) = self.keep_alive {
            body["keep_alive"] = ka.clone();
        }

        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format_ollama_error(&self.base_url, e)))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Ollama returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse Ollama response: {e}"))
        })?;

        let raw_content = json["message"]["content"].as_str().ok_or_else(|| {
            ThaiRagError::LlmProvider("Missing content in Ollama response".into())
        })?;

        // Strip <think>...</think> blocks (Qwen3, DeepSeek-R1, etc.)
        let content = thairag_core::strip_thinking_tags(raw_content);

        let usage = LlmUsage {
            prompt_tokens: json["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["eval_count"].as_u64().unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }
}

/// Format a reqwest error into a user-friendly message that distinguishes
/// connection failures from timeouts, helping operators diagnose the root cause.
fn format_ollama_error(base_url: &str, err: reqwest::Error) -> String {
    if err.is_connect() {
        format!(
            "Cannot connect to Ollama at {base_url}. \
             Is Ollama running? Check that the URL is correct and the server is reachable."
        )
    } else if err.is_timeout() {
        format!(
            "Ollama request timed out ({base_url}). \
             The model may be loading or the server is overloaded. \
             Try increasing request_timeout_secs in chat pipeline settings."
        )
    } else {
        format!("Ollama request failed ({base_url}): {err}")
    }
}

#[cfg(test)]
mod tests {
    use super::is_ollama_vision_model as v;
    use super::{MIN_NUM_CTX, OllamaProvider, bucket_num_ctx, estimate_message_tokens};
    use thairag_core::types::ChatMessage;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            images: vec![],
        }
    }

    fn provider(num_ctx_max: usize) -> OllamaProvider {
        OllamaProvider::with_options(
            "http://localhost:11434",
            "qwen2.5vl",
            5,
            None,
            num_ctx_max,
            None,
        )
    }

    fn provider_with_temp(temperature: Option<f32>) -> OllamaProvider {
        OllamaProvider::with_options(
            "http://localhost:11434",
            "qwen2.5vl",
            5,
            None,
            0,
            temperature,
        )
    }

    #[test]
    fn bucket_rounds_up_to_power_of_two_with_floor() {
        assert_eq!(bucket_num_ctx(0), MIN_NUM_CTX);
        assert_eq!(bucket_num_ctx(100), MIN_NUM_CTX); // floored then already a pow2
        assert_eq!(bucket_num_ctx(3000), 4096);
        assert_eq!(bucket_num_ctx(5000), 8192);
        assert_eq!(bucket_num_ctx(16384), 16384);
    }

    #[test]
    fn estimate_scales_with_content() {
        let small = estimate_message_tokens(&[msg("user", "hi")]);
        let big = estimate_message_tokens(&[msg("user", &"x".repeat(4000))]);
        assert!(big > small);
        // ~4 chars/token + 8 overhead: 4000/4 + ~3 ≈ 1003.
        assert!((1000..1100).contains(&big), "got {big}");
    }

    #[test]
    fn text_ctx_is_adaptive_and_clamped() {
        let p = provider(16384);
        // Short prompt → floored at MIN_NUM_CTX, not the cap.
        assert_eq!(
            p.text_num_ctx(&[msg("user", "hi")], Some(256)),
            Some(MIN_NUM_CTX)
        );
        // Large prompt is clamped to the cap, never exceeding it.
        let huge = msg("user", &"x".repeat(200_000));
        assert_eq!(p.text_num_ctx(&[huge], Some(1024)), Some(16384));
    }

    #[test]
    fn disabled_cap_omits_num_ctx() {
        let p = provider(0);
        assert_eq!(p.text_num_ctx(&[msg("user", "hi")], Some(256)), None);
        assert_eq!(p.vision_num_ctx(), None);
        // build_options still emits num_predict alone, mirroring prior behavior.
        let opts = p.build_options(Some(512), None).unwrap();
        assert_eq!(opts["num_predict"], 512);
        assert!(opts.get("num_ctx").is_none());
    }

    #[test]
    fn vision_requests_full_cap() {
        assert_eq!(provider(16384).vision_num_ctx(), Some(16384));
    }

    #[test]
    fn temperature_emitted_when_set() {
        let p = provider_with_temp(Some(0.2));
        let opts = p.build_options(Some(256), None).unwrap();
        // f32 0.2 widens to f64 in JSON; compare with tolerance.
        let temp = opts["temperature"].as_f64().unwrap();
        assert!((temp - 0.2).abs() < 1e-6);
        assert_eq!(opts["num_predict"], 256);
    }

    #[test]
    fn temperature_omitted_when_none() {
        let p = provider_with_temp(None);
        // Only temperature is unset here; num_ctx disabled and no max_tokens →
        // options omitted entirely, preserving model defaults.
        assert!(p.build_options(None, None).is_none());
        // When other options exist, temperature key is still absent.
        let opts = p.build_options(Some(256), None).unwrap();
        assert!(opts.get("temperature").is_none());
    }

    #[test]
    fn vision_model_recognition() {
        // Recognized vision models (incl. the previously-missed qwen3-vl).
        assert!(v("qwen3-vl:8b-instruct-bf16"));
        assert!(v("qwen2.5vl:latest"));
        assert!(v("qwen2-vl"));
        assert!(v("llava:13b"));
        assert!(v("llama3.2-vision:11b"));
        assert!(v("granite3.2-vision"));
        assert!(v("minicpm-v"));
        assert!(v("gemma3:4b"));
        // Not vision-capable.
        assert!(!v("llama3.2"));
        assert!(!v("nomic-embed-text"));
        assert!(!v("qwen2.5-coder")); // qwen, but no "vl"
    }
}
