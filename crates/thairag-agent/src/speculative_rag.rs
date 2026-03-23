use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmUsage};
use tracing::{debug, warn};

use crate::context_curator::CuratedContext;
use crate::query_analyzer::QueryAnalysis;

/// A candidate response with its quality score.
#[derive(Debug, Clone)]
pub struct CandidateResponse {
    pub text: String,
    pub quality_score: f32,
    pub strategy: String,
}

/// Speculative RAG: generates multiple candidate responses in parallel using
/// different prompting strategies, then selects the best one via LLM-based ranking.
pub struct SpeculativeRag {
    llm: Arc<dyn LlmProvider>,
    num_candidates: u32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl SpeculativeRag {
    pub fn new(llm: Arc<dyn LlmProvider>, num_candidates: u32, max_tokens: u32) -> Self {
        Self {
            llm,
            num_candidates,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        num_candidates: u32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            num_candidates,
            max_tokens,
            prompts,
        }
    }

    /// Generate multiple candidate responses with different strategies.
    pub async fn generate_candidates(
        &self,
        _analysis: &QueryAnalysis,
        context: &CuratedContext,
        messages: &[ChatMessage],
    ) -> Result<Vec<CandidateResponse>> {
        let context_text: String = context
            .chunks
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let strategies = self.pick_strategies();
        let num = (self.num_candidates as usize).min(strategies.len());

        // Generate candidates in parallel
        let mut handles = Vec::new();
        for (name, system_prompt) in strategies.into_iter().take(num) {
            let llm = Arc::clone(&self.llm);
            let context_text = context_text.clone();
            let msgs = messages.to_vec();
            let max_tokens = self.max_tokens;

            handles.push(tokio::spawn(async move {
                let mut prompt_msgs = Vec::with_capacity(msgs.len() + 1);
                prompt_msgs.push(ChatMessage {
                    role: "system".into(),
                    content: format!("{system_prompt}\n\nContext:\n{context_text}"),
                });
                // Add conversation history (skip any existing system messages)
                for m in &msgs {
                    if m.role != "system" {
                        prompt_msgs.push(m.clone());
                    }
                }

                match llm.generate(&prompt_msgs, Some(max_tokens)).await {
                    Ok(resp) => Some(CandidateResponse {
                        text: resp.content,
                        quality_score: 0.0, // scored later
                        strategy: name.to_string(),
                    }),
                    Err(e) => {
                        warn!(strategy = name, error = %e, "Speculative RAG: candidate generation failed");
                        None
                    }
                }
            }));
        }

        let mut candidates = Vec::new();
        for handle in handles {
            if let Ok(Some(candidate)) = handle.await {
                candidates.push(candidate);
            }
        }

        if candidates.is_empty() {
            warn!("Speculative RAG: no candidates generated, falling back");
            return Err(thairag_core::ThaiRagError::Internal(
                "Speculative RAG: all candidates failed".into(),
            ));
        }

        debug!(
            count = candidates.len(),
            "Speculative RAG: candidates generated"
        );
        Ok(candidates)
    }

    /// Rank candidates and return the best one.
    pub async fn select_best(
        &self,
        query: &str,
        candidates: &mut [CandidateResponse],
    ) -> Result<CandidateResponse> {
        if candidates.len() == 1 {
            return Ok(candidates[0].clone());
        }

        let candidate_texts: String = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "--- Candidate {} ({}) ---\n{}",
                    i + 1,
                    c.strategy,
                    truncate(&c.text, 500)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        const DEFAULT_RANKER: &str = r#"You are a response quality ranker. Given a query and multiple candidate responses,
rank them by quality. Consider: accuracy, completeness, clarity, and relevance to the query.

Return JSON: {"rankings": [{"candidate": 1, "score": 0.0-1.0, "reason": "brief"}], "best": 1}"#;

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.speculative_rag_ranker",
                DEFAULT_RANKER,
                &[],
            ),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}\n\n{candidate_texts}"),
        };

        match self.llm.generate(&[system, user], Some(256)).await {
            Ok(resp) => {
                let json_str = thairag_core::extract_json(resp.content.trim());
                match serde_json::from_str::<RankingOutput>(json_str) {
                    Ok(output) => {
                        // Apply scores
                        for ranking in &output.rankings {
                            let idx = ranking.candidate.saturating_sub(1) as usize;
                            if idx < candidates.len() {
                                candidates[idx].quality_score = ranking.score;
                            }
                        }
                        let best_idx =
                            (output.best.saturating_sub(1) as usize).min(candidates.len() - 1);
                        debug!(
                            best = best_idx + 1,
                            strategy = %candidates[best_idx].strategy,
                            score = candidates[best_idx].quality_score,
                            "Speculative RAG: best candidate selected"
                        );
                        Ok(candidates[best_idx].clone())
                    }
                    Err(_) => {
                        // Return the first candidate as fallback
                        Ok(candidates[0].clone())
                    }
                }
            }
            Err(_) => Ok(candidates[0].clone()),
        }
    }

    /// Generate candidates and select the best, returning as LlmResponse.
    pub async fn speculative_generate(
        &self,
        analysis: &QueryAnalysis,
        context: &CuratedContext,
        messages: &[ChatMessage],
        query: &str,
    ) -> Result<LlmResponse> {
        let mut candidates = self
            .generate_candidates(analysis, context, messages)
            .await?;
        let best = self.select_best(query, &mut candidates).await?;
        Ok(LlmResponse {
            content: best.text,
            usage: LlmUsage::default(),
        })
    }

    fn pick_strategies(&self) -> Vec<(&'static str, String)> {
        const DEFAULT_DETAILED: &str = "You are a knowledgeable assistant. Answer the query using the provided context. \
            Be thorough and detailed. Include citations [1][2] referencing context chunks.";
        const DEFAULT_CONCISE: &str = "You are a precise assistant. Answer the query using the provided context. \
            Be concise and direct — get to the point quickly. Include citations [1][2].";
        const DEFAULT_STEP_BY_STEP: &str = "You are an analytical assistant. Answer the query using the provided context. \
            Think step by step, explaining your reasoning. Include citations [1][2].";
        const DEFAULT_COMPARATIVE: &str = "You are a balanced assistant. Answer the query using the provided context. \
            If multiple viewpoints or data points exist, present them comparatively. Include citations [1][2].";

        vec![
            (
                "detailed",
                self.prompts.render_or_default(
                    "chat.speculative_rag_detailed",
                    DEFAULT_DETAILED,
                    &[],
                ),
            ),
            (
                "concise",
                self.prompts.render_or_default(
                    "chat.speculative_rag_concise",
                    DEFAULT_CONCISE,
                    &[],
                ),
            ),
            (
                "step_by_step",
                self.prompts.render_or_default(
                    "chat.speculative_rag_step_by_step",
                    DEFAULT_STEP_BY_STEP,
                    &[],
                ),
            ),
            (
                "comparative",
                self.prompts.render_or_default(
                    "chat.speculative_rag_comparative",
                    DEFAULT_COMPARATIVE,
                    &[],
                ),
            ),
        ]
    }
}

#[derive(Deserialize)]
struct RankingOutput {
    #[serde(default)]
    rankings: Vec<CandidateRanking>,
    #[serde(default = "default_one")]
    best: u32,
}

#[derive(Deserialize)]
struct CandidateRanking {
    candidate: u32,
    score: f32,
    #[serde(default)]
    #[allow(dead_code)]
    reason: String,
}

fn default_one() -> u32 {
    1
}

fn truncate(s: &str, max: usize) -> String {
    thairag_core::safe_truncate(s, max).to_string()
}
