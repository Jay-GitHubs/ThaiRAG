use std::sync::Arc;

use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{
    ChatMessage, OrchestratorAction, OrchestratorDecision, PipelineSnapshot,
};
use tracing::warn;

use super::analyzer::strip_json_fences;
use super::prompts;

/// LLM-powered pipeline orchestrator.
/// Reviews each agent's output and makes adaptive decisions:
/// accept, retry, adjust params, fallback, or flag for review.
pub struct LlmOrchestrator {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl LlmOrchestrator {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            prompts,
        }
    }

    /// Ask the orchestrator to decide what to do after a pipeline stage.
    pub async fn decide(&self, snapshot: &PipelineSnapshot) -> Result<OrchestratorDecision> {
        let prompt = prompts::orchestrator_prompt(&self.prompts, snapshot);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let json_str = strip_json_fences(response.content.trim());

        match serde_json::from_str::<OrchestratorDecision>(json_str) {
            Ok(decision) => Ok(decision),
            Err(e) => {
                warn!(error = %e, raw = %json_str.chars().take(200).collect::<String>(),
                    "Failed to parse orchestrator response, defaulting to Accept");
                Ok(OrchestratorDecision {
                    action: OrchestratorAction::Accept,
                    reasoning: format!("Parse error, auto-accepting: {e}"),
                    confidence: 0.0,
                })
            }
        }
    }
}
