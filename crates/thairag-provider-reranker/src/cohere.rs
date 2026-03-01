use std::time::Duration;

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::Reranker;
use thairag_core::types::SearchResult;
use thairag_core::ThaiRagError;
use tracing::{info, instrument};

pub struct CohereReranker {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl CohereReranker {
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client");

        info!(model, "Initialized Cohere reranker");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl Reranker for CohereReranker {
    #[instrument(skip(self, results), fields(model = %self.model, result_count = results.len()))]
    async fn rerank(&self, query: &str, results: Vec<SearchResult>) -> Result<Vec<SearchResult>> {
        if results.is_empty() {
            return Ok(results);
        }

        let documents: Vec<String> = results.iter().map(|r| r.chunk.content.clone()).collect();

        let body = serde_json::json!({
            "model": self.model,
            "query": query,
            "documents": documents,
            "top_n": results.len(),
            "return_documents": false,
        });

        let resp = self
            .client
            .post("https://api.cohere.com/v2/rerank")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::Internal(format!("Cohere rerank request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Internal(format!(
                "Cohere rerank returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ThaiRagError::Internal(format!("Failed to parse Cohere rerank response: {e}")))?;

        let reranked = json["results"]
            .as_array()
            .ok_or_else(|| ThaiRagError::Internal("Missing results in Cohere rerank response".into()))?;

        let mut scored_results: Vec<SearchResult> = reranked
            .iter()
            .filter_map(|item| {
                let index = item["index"].as_u64()? as usize;
                let relevance_score = item["relevance_score"].as_f64()? as f32;
                if index < results.len() {
                    let mut result = results[index].clone();
                    result.score = relevance_score;
                    Some(result)
                } else {
                    None
                }
            })
            .collect();

        // Sort by relevance score descending
        scored_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored_results)
    }
}
