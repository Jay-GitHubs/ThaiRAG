use std::time::Duration;

use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;
use tracing::{info, instrument};

pub struct CohereEmbeddingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    dimension: usize,
}

impl CohereEmbeddingProvider {
    pub fn new(api_key: &str, model: &str, dimension: usize) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, dimension, "Initialized Cohere embedding provider");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimension,
        }
    }
}

#[async_trait]
impl EmbeddingModel for CohereEmbeddingProvider {
    #[instrument(skip(self, texts), fields(model = %self.model, text_count = texts.len()))]
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let body = serde_json::json!({
            "model": self.model,
            "texts": texts,
            "input_type": "search_document",
            "embedding_types": ["float"],
        });

        let resp = self
            .client
            .post("https://api.cohere.com/v2/embed")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::Embedding(format!("Cohere embedding request failed: {e}"))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Embedding(format!(
                "Cohere embedding returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::Embedding(format!("Failed to parse Cohere embedding response: {e}"))
        })?;

        let float_embeddings = json["embeddings"]["float"].as_array().ok_or_else(|| {
            ThaiRagError::Embedding("Missing embeddings.float array in Cohere response".into())
        })?;

        let embeddings: Vec<Vec<f32>> = float_embeddings
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
