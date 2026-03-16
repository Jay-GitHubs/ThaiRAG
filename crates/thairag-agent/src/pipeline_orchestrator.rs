use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::query_analyzer::{Complexity, QueryAnalysis};

/// Route decision from the Pipeline Orchestrator.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineRoute {
    /// Greeting, thanks, meta, clarification — skip retrieval, use main LLM directly.
    DirectLlm,
    /// Simple retrieval — skip rewriter, run search → curator → generator.
    SimpleRetrieval,
    /// Full pipeline — rewriter → search → curator → generator → optional quality guard.
    FullPipeline,
    /// Complex pipeline — full pipeline with quality guard forced on.
    ComplexPipeline,
}

/// LLM response format for routing decisions.
#[derive(Deserialize)]
struct LlmRouteDecision {
    #[serde(default = "default_full")]
    route: String,
    #[serde(default)]
    reason: String,
}

fn default_full() -> String {
    "full_pipeline".into()
}

const DEFAULT_TEMPLATE: &str = "You are a pipeline orchestrator. Given query analysis, decide the optimal route.\n\
                Routes:\n\
                - direct_llm: No retrieval needed (greetings, thanks, meta questions, unclear queries)\n\
                - simple_retrieval: Simple fact lookup — skip query rewriting, search directly\n\
                - full_pipeline: Standard retrieval — rewrite query, search, curate, generate\n\
                - complex_pipeline: Complex multi-part question — full pipeline + quality verification\n\n\
                Decision factors:\n\
                - needs_context=false → direct_llm\n\
                - Simple + single topic → simple_retrieval\n\
                - Moderate complexity or comparison → full_pipeline\n\
                - Complex, multi-topic, or analysis → complex_pipeline\n\n\
                Output JSON only: {\"route\":\"...\",\"reason\":\"brief reason\"}";

pub struct PipelineOrchestrator {
    llm: Option<Arc<dyn LlmProvider>>,
    max_tokens: u32,
    budget: u32,
    prompts: Arc<PromptRegistry>,
}

impl PipelineOrchestrator {
    pub fn new(llm: Option<Arc<dyn LlmProvider>>, max_tokens: u32, budget: u32) -> Self {
        Self {
            llm,
            max_tokens,
            budget,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Option<Arc<dyn LlmProvider>>,
        max_tokens: u32,
        budget: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            budget,
            prompts,
        }
    }

    /// Decide which pipeline route to take based on the query analysis.
    pub async fn decide(&self, analysis: &QueryAnalysis) -> PipelineRoute {
        // Try LLM-based routing if available and budget > 0
        if let Some(ref llm) = self.llm && self.budget > 0 {
            match self.llm_decide(llm, analysis).await {
                Ok(route) => return route,
                Err(e) => {
                    warn!(error = %e, "Orchestrator LLM failed, falling back to heuristic");
                }
            }
        }

        // Heuristic fallback (zero-latency)
        heuristic_decide(analysis)
    }

    async fn llm_decide(
        &self,
        llm: &Arc<dyn LlmProvider>,
        analysis: &QueryAnalysis,
    ) -> Result<PipelineRoute> {
        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.pipeline_orchestrator",
                DEFAULT_TEMPLATE,
                &[],
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Query analysis:\n- language: {:?}\n- intent: {:?}\n- complexity: {:?}\n- topics: {:?}\n- needs_context: {}",
                analysis.language,
                analysis.intent,
                analysis.complexity,
                analysis.topics,
                analysis.needs_context,
            ),
        };

        let resp = llm.generate(&[system, user], Some(self.max_tokens)).await?;
        let content = resp.content.trim();

        // Extract JSON
        let json_str = extract_json(content);
        match serde_json::from_str::<LlmRouteDecision>(json_str) {
            Ok(decision) => {
                let route = parse_route(&decision.route);
                debug!(route = ?route, reason = %decision.reason, "Orchestrator LLM decided");
                Ok(route)
            }
            Err(e) => {
                warn!(error = %e, raw = %content, "Failed to parse orchestrator response");
                Ok(heuristic_decide(analysis))
            }
        }
    }
}

/// Zero-latency heuristic routing based on QueryAnalysis fields.
pub fn heuristic_decide(analysis: &QueryAnalysis) -> PipelineRoute {
    if !analysis.needs_context {
        return PipelineRoute::DirectLlm;
    }

    match analysis.complexity {
        Complexity::Simple => {
            if analysis.topics.len() <= 1 {
                PipelineRoute::SimpleRetrieval
            } else {
                PipelineRoute::FullPipeline
            }
        }
        Complexity::Moderate => PipelineRoute::FullPipeline,
        Complexity::Complex => PipelineRoute::ComplexPipeline,
    }
}

fn parse_route(s: &str) -> PipelineRoute {
    match s.trim().to_lowercase().as_str() {
        "direct_llm" | "direct" => PipelineRoute::DirectLlm,
        "simple_retrieval" | "simple" => PipelineRoute::SimpleRetrieval,
        "full_pipeline" | "full" => PipelineRoute::FullPipeline,
        "complex_pipeline" | "complex" => PipelineRoute::ComplexPipeline,
        _ => PipelineRoute::FullPipeline,
    }
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') && let Some(end) = s.rfind('}') {
        return &s[start..=end];
    }
    s
}
