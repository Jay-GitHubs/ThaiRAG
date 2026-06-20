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
    normalize_scores: bool,
}

impl JinaReranker {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_base_url(api_key, model, "", false)
    }

    /// `base_url` empty = Jina's public host. Any Jina-protocol-compatible
    /// `/v1/rerank` endpoint works (e.g. an OpenAI-compatible gateway's
    /// `rerank-bge`). A trailing `/v1` is tolerated and not duplicated.
    ///
    /// `normalize_scores`: when true, map each result's raw score through a
    /// sigmoid into `(0, 1)`. Jina's cloud API already returns 0–1 relevance,
    /// so leave this off for it; raw cross-encoder rerankers (e.g. a gateway's
    /// `rerank-bge`) emit unbounded logits that would otherwise trip downstream
    /// relevance thresholds — sigmoid is the canonical normalization and
    /// preserves ranking order.
    pub fn with_base_url(
        api_key: &str,
        model: &str,
        base_url: &str,
        normalize_scores: bool,
    ) -> Self {
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

        info!(model, %endpoint, normalize_scores, "Initialized Jina reranker");

        Self {
            client,
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint,
            normalize_scores,
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
                let raw = item["relevance_score"].as_f64()? as f32;
                // Sigmoid maps unbounded cross-encoder logits to (0, 1); it is
                // monotonic, so ranking is unchanged. Identity when off.
                let relevance_score = if self.normalize_scores {
                    1.0 / (1.0 + (-raw).exp())
                } else {
                    raw
                };
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
        assert!(!r.normalize_scores);

        // Gateway with trailing /v1 must not double.
        let r =
            JinaReranker::with_base_url("k", "rerank-bge", "https://llm.jay-tech-ai.com/v1", true);
        assert_eq!(r.endpoint, "https://llm.jay-tech-ai.com/v1/rerank");
        assert!(r.normalize_scores);

        // Bare host gets /v1/rerank appended.
        let r = JinaReranker::with_base_url("k", "rerank-bge", "https://host/", false);
        assert_eq!(r.endpoint, "https://host/v1/rerank");
    }

    #[test]
    fn sigmoid_normalizes_logits_to_unit_interval_preserving_order() {
        // The canonical BGE behaviour: a strong positive logit → near 1, a
        // strongly negative one → near 0, and ordering is preserved.
        let sigmoid = |raw: f32| 1.0 / (1.0 + (-raw).exp());
        let hit = sigmoid(3.28);
        let miss = sigmoid(-10.97);
        assert!(hit > 0.9, "relevant logit should map high, got {hit}");
        assert!(miss < 0.1, "irrelevant logit should map low, got {miss}");
        assert!(hit > miss);
    }
}
