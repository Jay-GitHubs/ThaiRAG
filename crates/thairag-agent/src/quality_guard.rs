use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::context_curator::CuratedContext;

/// Default hardcoded template for quality guard evaluation.
const DEFAULT_QUALITY_PROMPT: &str = "You are a response quality evaluator. Given a query, context chunks, and a generated response, \
evaluate the response quality.\n\n\
Output JSON only:\n\
{\"pass\":true|false,\
\"relevance\":0.0-1.0,\
\"hallucination\":0.0-1.0,\
\"completeness\":0.0-1.0,\
\"feedback\":\"specific improvement instructions or null\"}\n\n\
Scoring:\n\
- relevance: Does the response answer the query? (1.0 = perfectly relevant)\n\
- hallucination: Does the response contain info NOT in the context? (0.0 = no hallucination)\n\
- completeness: Does the response cover all relevant context? (1.0 = fully complete)\n\
- pass=false if relevance < {{threshold}} OR hallucination > {{hallucination_threshold}}\n\
- When pass=false, provide specific feedback for improvement\n\
Output ONLY valid JSON.";

/// Quality verdict from the guard.
#[derive(Debug, Clone)]
pub struct QualityVerdict {
    pub pass: bool,
    pub relevance_score: f32,
    pub hallucination_score: f32,
    pub completeness_score: f32,
    pub feedback: Option<String>,
}

#[derive(Deserialize)]
struct LlmVerdict {
    #[serde(default = "default_true")]
    pass: bool,
    #[serde(default = "default_high")]
    relevance: f32,
    #[serde(default = "default_low")]
    hallucination: f32,
    #[serde(default = "default_high")]
    completeness: f32,
    #[serde(default)]
    feedback: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_high() -> f32 {
    0.8
}
fn default_low() -> f32 {
    0.1
}

pub struct QualityGuard {
    llm: Arc<dyn LlmProvider>,
    threshold: f32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl QualityGuard {
    pub fn new(llm: Arc<dyn LlmProvider>, threshold: f32, max_tokens: u32) -> Self {
        Self {
            llm,
            threshold,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        threshold: f32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            threshold,
            max_tokens,
            prompts,
        }
    }

    pub async fn check(
        &self,
        query: &str,
        response: &str,
        context: &CuratedContext,
    ) -> Result<QualityVerdict> {
        self.check_with_threshold(query, response, context, self.threshold)
            .await
    }

    /// Check quality with an externally-provided threshold (for adaptive quality).
    pub async fn check_with_threshold(
        &self,
        query: &str,
        response: &str,
        context: &CuratedContext,
        threshold: f32,
    ) -> Result<QualityVerdict> {
        let context_text: String = context
            .chunks
            .iter()
            .map(|c| format!("[{}] {}", c.index, c.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let hallucination_threshold = 1.0 - self.threshold;

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.quality_guard",
                DEFAULT_QUALITY_PROMPT,
                &[
                    ("threshold", &threshold.to_string()),
                    (
                        "hallucination_threshold",
                        &hallucination_threshold.to_string(),
                    ),
                ],
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}\n\nContext:\n{context_text}\n\nResponse:\n{response}"),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<LlmVerdict>(json_str) {
                    Ok(v) => {
                        debug!(
                            pass = v.pass,
                            relevance = v.relevance,
                            hallucination = v.hallucination,
                            completeness = v.completeness,
                            "Quality guard verdict"
                        );
                        Ok(QualityVerdict {
                            pass: v.pass,
                            relevance_score: v.relevance,
                            hallucination_score: v.hallucination,
                            completeness_score: v.completeness,
                            feedback: v.feedback,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse quality verdict, passing");
                        Ok(QualityVerdict {
                            pass: true,
                            relevance_score: 0.8,
                            hallucination_score: 0.1,
                            completeness_score: 0.8,
                            feedback: None,
                        })
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Quality guard LLM failed, passing");
                Ok(QualityVerdict {
                    pass: true,
                    relevance_score: 0.8,
                    hallucination_score: 0.1,
                    completeness_score: 0.8,
                    feedback: None,
                })
            }
        }
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
