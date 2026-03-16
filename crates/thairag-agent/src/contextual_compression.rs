use std::sync::Arc;

use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::context_curator::CuratedContext;

/// Default hardcoded template for contextual compression.
const DEFAULT_COMPRESSION_PROMPT: &str = r#"You are a context compression expert. Given a query and a text passage, compress the passage to approximately {{target_pct}}% of its original length.

Rules:
1. Remove redundant information, filler words, and sentences that are NOT relevant to the query.
2. Preserve ALL facts, numbers, names, and key claims relevant to the query.
3. Maintain the original meaning — do NOT add new information.
4. Keep citations and references intact.
5. Return ONLY the compressed text, nothing else."#;

/// LLMLingua-style contextual compression: uses an LLM to identify and remove
/// low-importance tokens/sentences from context chunks, reducing context size
/// while preserving information density.
pub struct ContextualCompression {
    llm: Arc<dyn LlmProvider>,
    /// Target compression ratio (0.0-1.0). E.g., 0.5 means compress to ~50% of original.
    target_ratio: f32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl ContextualCompression {
    pub fn new(llm: Arc<dyn LlmProvider>, target_ratio: f32, max_tokens: u32) -> Self {
        Self {
            llm,
            target_ratio: target_ratio.clamp(0.1, 1.0),
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        target_ratio: f32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            target_ratio: target_ratio.clamp(0.1, 1.0),
            max_tokens,
            prompts,
        }
    }

    /// Compress a curated context by removing low-importance content.
    pub async fn compress(&self, query: &str, context: &CuratedContext) -> Result<CuratedContext> {
        if context.chunks.is_empty() {
            return Ok(context.clone());
        }

        let total_chars: usize = context.chunks.iter().map(|c| c.content.len()).sum();
        let target_chars = (total_chars as f32 * self.target_ratio) as usize;

        // Skip compression if context is already small enough
        if total_chars <= 2000 || self.target_ratio >= 0.95 {
            debug!(total_chars, "Compression: context small enough, skipping");
            return Ok(context.clone());
        }

        // Compress chunks in parallel
        let mut handles = Vec::new();
        for chunk in &context.chunks {
            let llm = Arc::clone(&self.llm);
            let prompts = Arc::clone(&self.prompts);
            let query = query.to_string();
            let content = chunk.content.clone();
            let ratio = self.target_ratio;
            let max_tokens = self.max_tokens;

            handles.push(tokio::spawn(async move {
                compress_chunk(&llm, &prompts, &query, &content, ratio, max_tokens).await
            }));
        }

        let mut compressed = context.clone();
        for (i, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(text)) => {
                    compressed.chunks[i].content = text;
                }
                Ok(Err(e)) => {
                    warn!(chunk = i, error = %e, "Compression: chunk compression failed, keeping original");
                }
                Err(e) => {
                    warn!(chunk = i, error = %e, "Compression: task panicked, keeping original");
                }
            }
        }

        let new_chars: usize = compressed.chunks.iter().map(|c| c.content.len()).sum();
        let actual_ratio = new_chars as f32 / total_chars as f32;
        compressed.total_tokens_est = (compressed.total_tokens_est as f32 * actual_ratio) as usize;

        debug!(
            original_chars = total_chars,
            compressed_chars = new_chars,
            target_chars,
            actual_ratio,
            "Compression: context compressed"
        );

        Ok(compressed)
    }
}

async fn compress_chunk(
    llm: &Arc<dyn LlmProvider>,
    prompts: &PromptRegistry,
    query: &str,
    content: &str,
    ratio: f32,
    max_tokens: u32,
) -> Result<String> {
    let target_pct = (ratio * 100.0) as u32;

    let system = ChatMessage {
        role: "system".into(),
        content: prompts.render_or_default(
            "chat.contextual_compression",
            DEFAULT_COMPRESSION_PROMPT,
            &[("target_pct", &target_pct.to_string())],
        ),
    };

    let user = ChatMessage {
        role: "user".into(),
        content: format!(
            "Query: {query}\n\nPassage to compress:\n{content}",
            content = truncate(content, 3000)
        ),
    };

    let resp = llm.generate(&[system, user], Some(max_tokens)).await?;
    let compressed = resp.content.trim().to_string();

    // Sanity check: don't use compression if it's longer than original or empty
    if compressed.is_empty() || compressed.len() > content.len() {
        return Ok(content.to_string());
    }

    Ok(compressed)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}
