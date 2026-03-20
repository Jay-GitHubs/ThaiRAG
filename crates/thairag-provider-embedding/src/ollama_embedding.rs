use std::time::Duration;

use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;
use tracing::{info, instrument};

pub struct OllamaEmbeddingProvider {
    client: reqwest::Client,
    model: String,
    dimension: usize,
    endpoint: String,
    keep_alive: Option<serde_json::Value>,
}

impl OllamaEmbeddingProvider {
    pub fn new(base_url: &str, model: &str, dimension: usize) -> Self {
        Self::with_keep_alive(base_url, model, dimension, None)
    }

    pub fn with_keep_alive(
        base_url: &str,
        model: &str,
        dimension: usize,
        keep_alive: Option<&str>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to build reqwest client");

        let base = base_url.trim_end_matches('/');
        let endpoint = format!("{base}/api/embed");

        let keep_alive_val = keep_alive.map(|s| {
            if let Ok(n) = s.parse::<i64>() {
                serde_json::Value::Number(serde_json::Number::from(n))
            } else {
                serde_json::Value::String(s.to_string())
            }
        });

        info!(model, dimension, %endpoint, ?keep_alive, "Initialized Ollama embedding provider");

        Self {
            client,
            model: model.to_string(),
            dimension,
            endpoint,
            keep_alive: keep_alive_val,
        }
    }
}

#[async_trait]
impl EmbeddingModel for OllamaEmbeddingProvider {
    #[instrument(skip(self, texts), fields(model = %self.model, text_count = texts.len()))]
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let mut body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        if let Some(ref ka) = self.keep_alive {
            body["keep_alive"] = ka.clone();
        }

        let resp = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                let msg = if e.is_connect() {
                    format!(
                        "Cannot connect to Ollama embedding service at {}. \
                         Is Ollama running? Check that the URL is correct.",
                        self.endpoint
                    )
                } else if e.is_timeout() {
                    format!(
                        "Ollama embedding request timed out ({}). \
                         The model may be loading or the server is overloaded.",
                        self.endpoint
                    )
                } else {
                    format!("Ollama embedding request failed: {e}")
                };
                ThaiRagError::Embedding(msg)
            })?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Embedding(format!(
                "Ollama embedding returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::Embedding(format!("Failed to parse Ollama embedding response: {e}"))
        })?;

        let embeddings_arr = json["embeddings"].as_array().ok_or_else(|| {
            ThaiRagError::Embedding("Missing embeddings array in Ollama response".into())
        })?;

        let embeddings: Vec<Vec<f32>> = embeddings_arr
            .iter()
            .map(|arr| {
                arr.as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
            .collect();

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
