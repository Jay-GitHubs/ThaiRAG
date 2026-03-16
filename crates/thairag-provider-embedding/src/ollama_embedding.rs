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
}

impl OllamaEmbeddingProvider {
    pub fn new(base_url: &str, model: &str, dimension: usize) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build reqwest client");

        let base = base_url.trim_end_matches('/');
        let endpoint = format!("{base}/api/embed");

        info!(model, dimension, %endpoint, "Initialized Ollama embedding provider");

        Self {
            client,
            model: model.to_string(),
            dimension,
            endpoint,
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

        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });

        let resp = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::Embedding(format!("Ollama embedding request failed: {e}"))
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
