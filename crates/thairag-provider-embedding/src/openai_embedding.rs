use std::time::Duration;

use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::EmbeddingModel;
use tracing::{info, instrument};

pub struct OpenAiEmbeddingProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    dimension: usize,
    endpoint: String,
}

impl OpenAiEmbeddingProvider {
    pub fn new(api_key: &str, model: &str, dimension: usize, base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build reqwest client");

        let base = if base_url.is_empty() {
            "https://api.openai.com"
        } else {
            base_url.trim_end_matches('/')
        };
        let endpoint = format!("{base}/v1/embeddings");

        info!(model, dimension, %endpoint, "Initialized OpenAI embedding provider");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimension,
            endpoint,
        }
    }
}

#[async_trait]
impl EmbeddingModel for OpenAiEmbeddingProvider {
    #[instrument(skip(self, texts), fields(model = %self.model, text_count = texts.len()))]
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
            "dimensions": self.dimension,
        });

        let resp = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ThaiRagError::Embedding(format!("OpenAI embedding request failed: {e}"))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Embedding(format!(
                "OpenAI embedding returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::Embedding(format!("Failed to parse OpenAI embedding response: {e}"))
        })?;

        let data = json["data"].as_array().ok_or_else(|| {
            ThaiRagError::Embedding("Missing data array in OpenAI embedding response".into())
        })?;

        let mut embeddings: Vec<(usize, Vec<f32>)> = data
            .iter()
            .map(|item| {
                let index = item["index"].as_u64().unwrap_or(0) as usize;
                let embedding: Vec<f32> = item["embedding"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                (index, embedding)
            })
            .collect();

        // Sort by index to match input order
        embeddings.sort_by_key(|(i, _)| *i);

        Ok(embeddings.into_iter().map(|(_, emb)| emb).collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
