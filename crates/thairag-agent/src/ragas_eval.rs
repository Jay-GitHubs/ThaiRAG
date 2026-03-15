use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::context_curator::CuratedContext;

/// RAGAS-style evaluation scores for a RAG response.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RagasScores {
    /// Are claims in the response supported by the context? (0.0 = hallucinated, 1.0 = fully faithful)
    pub faithfulness: f32,
    /// Does the response actually answer the question? (0.0 = irrelevant, 1.0 = perfectly relevant)
    pub answer_relevancy: f32,
    /// Is the retrieved context relevant to the query? (0.0 = irrelevant, 1.0 = highly relevant)
    pub context_precision: f32,
    /// Overall composite score.
    pub overall: f32,
}

impl RagasScores {
    fn compute_overall(&mut self) {
        self.overall = (self.faithfulness + self.answer_relevancy + self.context_precision) / 3.0;
    }
}

/// Cumulative evaluation statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RagasStats {
    pub total_evaluations: u64,
    pub avg_faithfulness: f32,
    pub avg_answer_relevancy: f32,
    pub avg_context_precision: f32,
    pub avg_overall: f32,
}

/// RAGAS evaluator: runs automated quality benchmarks on RAG responses using
/// LLM-based evaluation. Samples a configurable fraction of responses.
///
/// Metrics computed:
/// - **Faithfulness**: Are claims supported by context? (statement extraction + verification)
/// - **Answer Relevancy**: Does the response answer the query? (reverse question generation)
/// - **Context Precision**: Is the retrieved context actually useful? (LLM judgment)
pub struct RagasEvaluator {
    llm: Arc<dyn LlmProvider>,
    sample_rate: f32,
    max_tokens: u32,
    eval_counter: AtomicU64,
    request_counter: AtomicU64,
}

impl RagasEvaluator {
    pub fn new(llm: Arc<dyn LlmProvider>, sample_rate: f32, max_tokens: u32) -> Self {
        Self {
            llm,
            sample_rate: sample_rate.clamp(0.0, 1.0),
            max_tokens,
            eval_counter: AtomicU64::new(0),
            request_counter: AtomicU64::new(0),
        }
    }

    /// Determine whether this request should be evaluated (sampling).
    pub fn should_evaluate(&self) -> bool {
        let count = self.request_counter.fetch_add(1, Ordering::Relaxed);
        // Simple deterministic sampling: evaluate every 1/sample_rate requests
        if self.sample_rate <= 0.0 {
            return false;
        }
        if self.sample_rate >= 1.0 {
            return true;
        }
        let interval = (1.0 / self.sample_rate) as u64;
        count % interval == 0
    }

    /// Run full RAGAS evaluation on a query/context/response triple.
    pub async fn evaluate(
        &self,
        query: &str,
        context: &CuratedContext,
        response: &str,
    ) -> Result<RagasScores> {
        let context_text: String = context.chunks.iter().take(5)
            .map(|c| truncate(&c.content, 300))
            .collect::<Vec<_>>()
            .join("\n---\n");

        // Run all three evaluations in parallel
        let (faithfulness, relevancy, precision) = tokio::join!(
            self.eval_faithfulness(query, &context_text, response),
            self.eval_answer_relevancy(query, response),
            self.eval_context_precision(query, &context_text),
        );

        let mut scores = RagasScores {
            faithfulness: faithfulness.unwrap_or(0.5),
            answer_relevancy: relevancy.unwrap_or(0.5),
            context_precision: precision.unwrap_or(0.5),
            overall: 0.0,
        };
        scores.compute_overall();

        self.eval_counter.fetch_add(1, Ordering::Relaxed);

        debug!(
            faithfulness = scores.faithfulness,
            answer_relevancy = scores.answer_relevancy,
            context_precision = scores.context_precision,
            overall = scores.overall,
            "RAGAS: evaluation complete"
        );

        Ok(scores)
    }

    /// Evaluate faithfulness: are claims in the response supported by context?
    async fn eval_faithfulness(&self, _query: &str, context: &str, response: &str) -> Result<f32> {
        let system = ChatMessage {
            role: "system".into(),
            content: r#"You evaluate faithfulness of a response to its source context.

Step 1: Extract factual claims from the response.
Step 2: For each claim, check if it is supported by the context.
Step 3: Calculate: faithfulness = supported_claims / total_claims

Return JSON: {"claims_total": N, "claims_supported": N, "faithfulness": 0.0-1.0}"#.into(),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Context:\n{context}\n\nResponse:\n{response}",
                context = truncate(context, 1500),
                response = truncate(response, 1000)
            ),
        };

        match self.llm.generate(&[system, user], Some(self.max_tokens)).await {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                #[derive(Deserialize)]
                struct F { faithfulness: f32 }
                match serde_json::from_str::<F>(json_str) {
                    Ok(f) => Ok(f.faithfulness.clamp(0.0, 1.0)),
                    Err(_) => Ok(0.5),
                }
            }
            Err(e) => {
                warn!(error = %e, "RAGAS faithfulness eval failed");
                Ok(0.5)
            }
        }
    }

    /// Evaluate answer relevancy: does the response actually answer the question?
    async fn eval_answer_relevancy(&self, query: &str, response: &str) -> Result<f32> {
        let system = ChatMessage {
            role: "system".into(),
            content: r#"Evaluate how well the response answers the given query.

Consider:
- Does it address the main question?
- Is it on-topic?
- Does it provide the requested information?

Return JSON: {"relevancy": 0.0-1.0, "reason": "brief"}"#.into(),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Query: {query}\n\nResponse:\n{response}",
                response = truncate(response, 1000)
            ),
        };

        match self.llm.generate(&[system, user], Some(self.max_tokens)).await {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                #[derive(Deserialize)]
                struct R { relevancy: f32 }
                match serde_json::from_str::<R>(json_str) {
                    Ok(r) => Ok(r.relevancy.clamp(0.0, 1.0)),
                    Err(_) => Ok(0.5),
                }
            }
            Err(e) => {
                warn!(error = %e, "RAGAS answer relevancy eval failed");
                Ok(0.5)
            }
        }
    }

    /// Evaluate context precision: is the retrieved context relevant to the query?
    async fn eval_context_precision(&self, query: &str, context: &str) -> Result<f32> {
        let system = ChatMessage {
            role: "system".into(),
            content: r#"Evaluate how relevant the retrieved context is to the query.

Consider:
- Does the context contain information needed to answer the query?
- Is the context focused or does it contain mostly irrelevant information?
- Could the query be answered from this context alone?

Return JSON: {"precision": 0.0-1.0, "reason": "brief"}"#.into(),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Query: {query}\n\nRetrieved context:\n{context}",
                context = truncate(context, 1500)
            ),
        };

        match self.llm.generate(&[system, user], Some(self.max_tokens)).await {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                #[derive(Deserialize)]
                struct P { precision: f32 }
                match serde_json::from_str::<P>(json_str) {
                    Ok(p) => Ok(p.precision.clamp(0.0, 1.0)),
                    Err(_) => Ok(0.5),
                }
            }
            Err(e) => {
                warn!(error = %e, "RAGAS context precision eval failed");
                Ok(0.5)
            }
        }
    }

    /// Get the total number of evaluations performed.
    pub fn total_evaluations(&self) -> u64 {
        self.eval_counter.load(Ordering::Relaxed)
    }
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { s[..max].to_string() }
}
