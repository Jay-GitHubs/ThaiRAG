use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, SearchResult};
use tracing::debug;

/// Default hardcoded template for ColBERT-style passage scoring.
const DEFAULT_COLBERT_PROMPT: &str = r#"You are a fine-grained relevance scorer. Given a query and a passage, evaluate relevance across multiple aspects:

1. **Exact Match**: Does the passage contain exact terms/phrases from the query?
2. **Semantic Match**: Does the passage address the query's intent, even with different wording?
3. **Completeness**: How much of the query is covered by the passage?
4. **Specificity**: Is the information specific and detailed, or vague?

Return JSON: {"exact_match": 0.0-1.0, "semantic_match": 0.0-1.0, "completeness": 0.0-1.0, "specificity": 0.0-1.0, "overall": 0.0-1.0}"#;

/// ColBERT-style late interaction reranker: instead of single-vector similarity,
/// this uses an LLM to perform fine-grained token-level relevance assessment
/// between the query and each search result. This simulates the "late interaction"
/// paradigm where query tokens individually attend to document tokens.
///
/// Since we don't have actual ColBERT embeddings, we approximate the concept
/// using LLM-based passage scoring with explicit per-aspect relevance evaluation.
pub struct ColbertReranker {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    /// Number of top results to rerank (skip the rest).
    top_n: usize,
    prompts: Arc<PromptRegistry>,
}

impl ColbertReranker {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32, top_n: usize) -> Self {
        Self {
            llm,
            max_tokens,
            top_n,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        top_n: usize,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            top_n,
            prompts,
        }
    }

    /// Rerank search results using fine-grained LLM-based scoring.
    /// Returns results sorted by the new scores.
    pub async fn rerank(&self, query: &str, results: &[SearchResult]) -> Result<Vec<SearchResult>> {
        if results.len() <= 1 {
            return Ok(results.to_vec());
        }

        let n = self.top_n.min(results.len());
        let to_rerank = &results[..n];
        let rest = &results[n..];

        // Score each result in parallel
        let mut handles = Vec::new();
        for (i, result) in to_rerank.iter().enumerate() {
            let llm = Arc::clone(&self.llm);
            let prompts = Arc::clone(&self.prompts);
            let query = query.to_string();
            let content = result.chunk.content.clone();
            let original_score = result.score;
            let max_tokens = self.max_tokens;

            handles.push(tokio::spawn(async move {
                match score_passage(&llm, &prompts, &query, &content, max_tokens).await {
                    Ok(llm_score) => {
                        // Blend: 40% original score + 60% LLM score
                        let blended = original_score * 0.4 + llm_score * 0.6;
                        (i, blended)
                    }
                    Err(_) => (i, original_score), // fallback to original
                }
            }));
        }

        let mut scored: Vec<(usize, f32)> = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                scored.push(result);
            }
        }

        // Sort by new scores descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut reranked: Vec<SearchResult> = Vec::with_capacity(results.len());
        for (idx, new_score) in &scored {
            let mut r = to_rerank[*idx].clone();
            r.score = *new_score;
            reranked.push(r);
        }

        // Append remaining results unchanged
        reranked.extend_from_slice(rest);

        debug!(
            reranked = n,
            total = results.len(),
            "ColBERT reranker: reranked top results"
        );

        Ok(reranked)
    }
}

/// Score a passage against a query using multi-aspect LLM evaluation.
async fn score_passage(
    llm: &Arc<dyn LlmProvider>,
    prompts: &PromptRegistry,
    query: &str,
    passage: &str,
    max_tokens: u32,
) -> Result<f32> {
    let system = ChatMessage {
        role: "system".into(),
        content: prompts.render_or_default("chat.colbert_reranker", DEFAULT_COLBERT_PROMPT, &[]),
    };

    let user = ChatMessage {
        role: "user".into(),
        content: format!("Query: {query}\n\nPassage:\n{}", truncate(passage, 1500)),
    };

    let resp = llm.generate(&[system, user], Some(max_tokens)).await?;
    let json_str = thairag_core::extract_json(resp.content.trim());

    #[derive(Deserialize)]
    struct Score {
        #[serde(default = "default_half")]
        overall: f32,
    }

    match serde_json::from_str::<Score>(json_str) {
        Ok(s) => Ok(s.overall.clamp(0.0, 1.0)),
        Err(_) => Ok(0.5),
    }
}

fn default_half() -> f32 {
    0.5
}

fn truncate(s: &str, max: usize) -> String {
    thairag_core::safe_truncate(s, max).to_string()
}
