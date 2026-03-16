use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmStreamResponse, LlmUsage, VisionMessage};
use tracing::{info, instrument};

pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl GeminiProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, "Initialized Gemini LLM provider");

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
    ) -> serde_json::Value {
        // Extract system messages into system_instruction
        let system_text: Option<String> = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .reduce(|mut acc, s| {
                acc.push('\n');
                acc.push_str(&s);
                acc
            });

        // Map non-system messages; Gemini uses "user" and "model" (not "assistant")
        let contents: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = if m.role == "assistant" {
                    "model"
                } else {
                    &m.role
                };
                serde_json::json!({
                    "role": role,
                    "parts": [{"text": &m.content}],
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "contents": contents,
        });

        if let Some(system) = system_text {
            body["system_instruction"] = serde_json::json!({
                "parts": [{"text": system}],
            });
        }

        if let Some(max) = max_tokens {
            body["generationConfig"] = serde_json::json!({
                "maxOutputTokens": max,
            });
        }

        body
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let body = self.build_request_body(messages, max_tokens);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Gemini request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Gemini returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse Gemini response: {e}"))
        })?;

        let content = json["candidates"][0]["content"]["parts"]
            .as_array()
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing parts in Gemini response".into()))?
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("");

        if content.is_empty() {
            return Err(ThaiRagError::LlmProvider(
                "No text content in Gemini response".into(),
            ));
        }

        let usage = LlmUsage {
            prompt_tokens: json["usageMetadata"]["promptTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            completion_tokens: json["usageMetadata"]["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }

    #[instrument(skip(self, messages), fields(model = %self.model, msg_count = messages.len()))]
    async fn generate_stream(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmStreamResponse> {
        let body = self.build_request_body(messages, max_tokens);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model, self.api_key
        );

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Gemini stream request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Gemini returned HTTP {status}: {error_body}"
            )));
        }

        let usage_cell: Arc<Mutex<Option<LlmUsage>>> = Arc::new(Mutex::new(None));
        let usage_writer = Arc::clone(&usage_cell);

        use tokio_stream::StreamExt;
        let mut byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buf = String::new();
            let mut total_prompt: u32 = 0;
            let mut total_completion: u32 = 0;
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk
                    .map_err(|e| ThaiRagError::LlmProvider(format!("Gemini stream read error: {e}")))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                // Gemini SSE: "data: <json>\n\n"
                while let Some(double_newline) = buf.find("\n\n") {
                    let line = buf[..double_newline].trim().to_string();
                    buf = buf[double_newline + 2..].to_string();

                    let Some(data) = line.strip_prefix("data: ") else { continue };

                    let json: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Extract text from candidates
                    if let Some(parts) = json["candidates"][0]["content"]["parts"].as_array() {
                        for part in parts {
                            if let Some(text) = part["text"].as_str()
                                && !text.is_empty()
                            {
                                yield text.to_string();
                            }
                        }
                    }

                    // Track usage from each chunk (last one has final counts)
                    if let Some(usage) = json.get("usageMetadata") {
                        if let Some(pt) = usage["promptTokenCount"].as_u64() {
                            total_prompt = pt as u32;
                        }
                        if let Some(ct) = usage["candidatesTokenCount"].as_u64() {
                            total_completion = ct as u32;
                        }
                    }
                }
            }
            // Write final usage
            *usage_writer.lock().unwrap() = Some(LlmUsage {
                prompt_tokens: total_prompt,
                completion_tokens: total_completion,
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
        // All Gemini 1.5+ and 2.x models support vision
        let m = &self.model;
        m.contains("gemini-1.5") || m.contains("gemini-2")
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

        let contents: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = if m.role == "assistant" {
                    "model"
                } else {
                    &m.role
                };
                let mut parts: Vec<serde_json::Value> = Vec::new();
                // Add images
                for img in &m.images {
                    parts.push(serde_json::json!({
                        "inline_data": {
                            "mime_type": img.media_type,
                            "data": img.base64_data,
                        }
                    }));
                }
                // Add text
                if !m.text.is_empty() {
                    parts.push(serde_json::json!({"text": &m.text}));
                }
                serde_json::json!({
                    "role": role,
                    "parts": parts,
                })
            })
            .collect();

        let mut body = serde_json::json!({ "contents": contents });

        if let Some(system) = system_text {
            body["system_instruction"] = serde_json::json!({
                "parts": [{"text": system}],
            });
        }

        if let Some(max) = max_tokens {
            body["generationConfig"] = serde_json::json!({ "maxOutputTokens": max });
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::LlmProvider(format!("Gemini vision request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::LlmProvider(format!(
                "Gemini returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::LlmProvider(format!("Failed to parse Gemini response: {e}"))
        })?;

        let content = json["candidates"][0]["content"]["parts"]
            .as_array()
            .ok_or_else(|| ThaiRagError::LlmProvider("Missing parts in Gemini response".into()))?
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("");

        if content.is_empty() {
            return Err(ThaiRagError::LlmProvider(
                "No text content in Gemini vision response".into(),
            ));
        }

        let usage = LlmUsage {
            prompt_tokens: json["usageMetadata"]["promptTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            completion_tokens: json["usageMetadata"]["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
        };

        Ok(LlmResponse { content, usage })
    }
}
