use std::time::Duration;

use async_trait::async_trait;
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::Reranker;
use thairag_core::types::SearchResult;
use tracing::{info, instrument};

pub struct JinaReranker {
    client: reqwest::Client,
    api_key: String,
    model: String,
    endpoint: String,
}

impl JinaReranker {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_base_url(api_key, model, "")
    }

    /// `base_url` empty = Jina's public host. Any Jina-protocol-compatible
    /// `/v1/rerank` endpoint works (e.g. an OpenAI-compatible gateway's
    /// `rerank-bge`). A trailing `/v1` is tolerated and not duplicated.
    pub fn with_base_url(api_key: &str, model: &str, base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client");

        let base = if base_url.is_empty() {
            "https://api.jina.ai"
        } else {
            let trimmed = base_url.trim_end_matches('/');
            trimmed.strip_suffix("/v1").unwrap_or(trimmed)
        };
        let endpoint = format!("{base}/v1/rerank");

        info!(model, %endpoint, "Initialized Jina reranker");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint,
        }
    }
}

#[async_trait]
impl Reranker for JinaReranker {
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
        });

        let resp = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ThaiRagError::Internal(format!("Jina rerank request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ThaiRagError::Internal(format!(
                "Jina rerank returned HTTP {status}: {error_body}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ThaiRagError::Internal(format!("Failed to parse Jina rerank response: {e}"))
        })?;

        let reranked = json["results"].as_array().ok_or_else(|| {
            ThaiRagError::Internal("Missing results in Jina rerank response".into())
        })?;

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
        scored_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(scored_results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_targets_v1_rerank_without_doubling() {
        // Default host when no base_url given.
        let r = JinaReranker::new("k", "jina-reranker-v2");
        assert_eq!(r.endpoint, "https://api.jina.ai/v1/rerank");

        // Gateway with trailing /v1 must not double.
        let r = JinaReranker::with_base_url("k", "rerank-bge", "https://llm.jay-tech-ai.com/v1");
        assert_eq!(r.endpoint, "https://llm.jay-tech-ai.com/v1/rerank");

        // Bare host gets /v1/rerank appended.
        let r = JinaReranker::with_base_url("k", "rerank-bge", "https://host/");
        assert_eq!(r.endpoint, "https://host/v1/rerank");
    }
}
