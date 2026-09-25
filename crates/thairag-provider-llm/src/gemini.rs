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
    temperature: Option<f32>,
    sampling: thairag_core::types::SamplingParams,
    reasoning: thairag_core::types::ReasoningParams,
}

impl GeminiProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_timeout(api_key, model, 120)
    }

    pub fn with_timeout(api_key: &str, model: &str, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, timeout_secs, "Initialized Gemini LLM provider");

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

    /// Build `generationConfig` from max_tokens + the configured sampling
    /// (Gemini names). Omitted entirely when nothing is set.
    fn generation_config(&self, max_tokens: Option<u32>) -> Option<serde_json::Value> {
        let s = &self.sampling;
        let mut cfg = serde_json::Map::new();
        if let Some(max) = max_tokens {
            cfg.insert("maxOutputTokens".into(), serde_json::json!(max));
        }
        if let Some(t) = self.temperature {
            cfg.insert(
                "temperature".into(),
                serde_json::json!(thairag_core::types::f32_as_json_number(t)),
            );
        }
        if let Some(v) = s.top_p {
            cfg.insert("topP".into(), serde_json::json!(v));
        }
        if let Some(v) = s.top_k {
            cfg.insert("topK".into(), serde_json::json!(v));
        }
        if let Some(v) = s.seed {
            cfg.insert("seed".into(), serde_json::json!(v));
        }
        if let Some(v) = s.presence_penalty {
            cfg.insert("presencePenalty".into(), serde_json::json!(v));
        }
        if let Some(v) = s.frequency_penalty {
            cfg.insert("frequencyPenalty".into(), serde_json::json!(v));
        }
        if !s.stop.is_empty() {
            cfg.insert("stopSequences".into(), serde_json::json!(s.stop));
        }
        // thinkingConfig: off → budget 0; on without a budget → -1 (dynamic);
        // a budget alone sets it. `reasoning_effort` has no Gemini mapping.
        let r = &self.reasoning;
        let thinking_budget: Option<i64> = match (r.thinking, r.thinking_budget_tokens) {
            (Some(false), _) => Some(0),
            (_, Some(b)) => Some(b as i64),
            (Some(true), None) => Some(-1),
            (None, None) => None,
        };
        if let Some(b) = thinking_budget {
            cfg.insert(
                "thinkingConfig".into(),
                serde_json::json!({ "thinkingBudget": b }),
            );
        }
        let mut value = serde_json::Value::Object(cfg);
        s.merge_extra_into(&mut value);
        if value.as_object().is_some_and(|o| o.is_empty()) {
            None
        } else {
            Some(value)
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

        if let Some(cfg) = self.generation_config(max_tokens) {
            body["generationConfig"] = cfg;
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

        if let Some(cfg) = self.generation_config(max_tokens) {
            body["generationConfig"] = cfg;
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

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::SamplingParams;

    #[test]
    fn thinking_config_maps_toggle_and_budget() {
        use thairag_core::types::ReasoningParams;
        let off = GeminiProvider::new("k", "g").with_reasoning(ReasoningParams {
            thinking: Some(false),
            ..Default::default()
        });
        assert_eq!(
            off.generation_config(None).unwrap()["thinkingConfig"]["thinkingBudget"],
            0
        );
        let dynamic = GeminiProvider::new("k", "g").with_reasoning(ReasoningParams {
            thinking: Some(true),
            ..Default::default()
        });
        assert_eq!(
            dynamic.generation_config(None).unwrap()["thinkingConfig"]["thinkingBudget"],
            -1
        );
        let budget = GeminiProvider::new("k", "g").with_reasoning(ReasoningParams {
            thinking_budget_tokens: Some(1024),
            ..Default::default()
        });
        assert_eq!(
            budget.generation_config(None).unwrap()["thinkingConfig"]["thinkingBudget"],
            1024
        );
    }

    #[test]
    fn generation_config_uses_gemini_names_and_is_omitted_when_empty() {
        let p = GeminiProvider::new("k", "gemini-x");
        assert!(p.generation_config(None).is_none());
        let p = p.with_sampling(
            Some(0.1),
            SamplingParams {
                top_p: Some(0.8),
                top_k: Some(32),
                seed: Some(3),
                stop: vec!["END".into()],
                ..Default::default()
            },
        );
        let cfg = p.generation_config(Some(100)).unwrap();
        assert_eq!(cfg["maxOutputTokens"], 100);
        assert!((cfg["temperature"].as_f64().unwrap() - 0.1).abs() < 1e-6);
        assert!((cfg["topP"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(cfg["topK"], 32);
        assert_eq!(cfg["seed"], 3);
        assert_eq!(cfg["stopSequences"], serde_json::json!(["END"]));
    }
}
