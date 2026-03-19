use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, VisionMessage};
use tracing::{info, instrument};

pub struct OllamaProvider {
    client: reqwest::Client,
    /// Separate client for streaming — no overall timeout so long generations aren't killed.
    stream_client: reqwest::Client,
    base_url: String,
    model: String,
    keep_alive: Option<serde_json::Value>,
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
            "Initialized Ollama provider"
        );

        Self {
            client,
            stream_client,
            base_url: base_url.to_string(),
            model: model.to_string(),
            keep_alive: keep_alive_val,
        }
    }
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

        if let Some(num_predict) = max_tokens {
            body["options"] = serde_json::json!({ "num_predict": num_predict });
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

        let content = json["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                ThaiRagError::LlmProvider("Missing content in Ollama response".into())
            })?;

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

        if let Some(num_predict) = max_tokens {
            body["options"] = serde_json::json!({ "num_predict": num_predict });
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
        // Ollama vision models
        let m = self.model.to_lowercase();
        m.contains("llava")
            || m.contains("llama3.2-vision")
            || m.contains("minicpm-v")
            || m.contains("bakllava")
            || m.contains("moondream")
            || m.contains("cogvlm")
            || m.contains("internvl")
            || m.contains("qwen2.5vl")
            || m.contains("qwen2-vl")
            || m.contains("qwenvl")
            || m.contains("gemma3")
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

        if let Some(num_predict) = max_tokens {
            body["options"] = serde_json::json!({ "num_predict": num_predict });
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

        let content = json["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                ThaiRagError::LlmProvider("Missing content in Ollama response".into())
            })?;

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
