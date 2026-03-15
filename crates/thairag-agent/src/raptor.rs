use std::sync::Arc;

use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use thairag_core::PromptRegistry;
use tracing::{debug, warn};

use crate::context_curator::{CuratedChunk, CuratedContext};

/// RAPTOR (Recursive Abstractive Processing for Tree-Organized Retrieval):
/// Builds a hierarchical tree of summaries over retrieved chunks. Lower levels
/// contain the original chunks; higher levels contain progressively more abstract
/// summaries. This allows the pipeline to answer both detail-oriented and
/// high-level synthesis questions from the same retrieval results.
pub struct Raptor {
    llm: Arc<dyn LlmProvider>,
    /// Maximum tree depth (levels of summarization above leaf chunks).
    max_depth: u32,
    /// Maximum chunks per summary group at each level.
    group_size: usize,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl Raptor {
    pub fn new(llm: Arc<dyn LlmProvider>, max_depth: u32, group_size: usize, max_tokens: u32) -> Self {
        Self {
            llm,
            max_depth: max_depth.max(1),
            group_size: group_size.max(2),
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(llm: Arc<dyn LlmProvider>, max_depth: u32, group_size: usize, max_tokens: u32, prompts: Arc<PromptRegistry>) -> Self {
        Self {
            llm,
            max_depth: max_depth.max(1),
            group_size: group_size.max(2),
            max_tokens,
            prompts,
        }
    }

    /// Build a RAPTOR tree from curated context and return an enriched context
    /// that includes both original chunks and hierarchical summaries.
    pub async fn build_tree(
        &self,
        query: &str,
        context: &CuratedContext,
    ) -> Result<CuratedContext> {
        if context.chunks.len() <= 2 {
            return Ok(context.clone());
        }

        let mut enriched = context.clone();
        let mut current_level: Vec<String> = context.chunks.iter()
            .map(|c| c.content.clone())
            .collect();

        let mut level = 0u32;
        while level < self.max_depth && current_level.len() > 1 {
            let groups = chunk_into_groups(&current_level, self.group_size);
            if groups.len() <= 1 && level > 0 {
                break; // Already at root
            }

            let mut next_level = Vec::new();
            let mut handles = Vec::new();

            for group in groups {
                let llm = Arc::clone(&self.llm);
                let query = query.to_string();
                let max_tokens = self.max_tokens;
                let level_num = level;
                let prompts = Arc::clone(&self.prompts);

                handles.push(tokio::spawn(async move {
                    summarize_group(&llm, &query, &group, level_num, max_tokens, &prompts).await
                }));
            }

            for handle in handles {
                match handle.await {
                    Ok(Ok(summary)) => {
                        next_level.push(summary.clone());
                        // Add summary as a high-level context chunk
                        let depth_label = match level {
                            0 => "section summary",
                            1 => "topic summary",
                            _ => "overview",
                        };
                        enriched.chunks.push(CuratedChunk {
                            index: enriched.chunks.len() + 1,
                            content: format!("[{depth_label}] {summary}"),
                            relevance_score: 0.4 + (level as f32 * 0.1), // higher levels get moderate scores
                            source_doc_id: Default::default(),
                            source_chunk_id: Default::default(),
                        });
                    }
                    Ok(Err(e)) => {
                        warn!(level, error = %e, "RAPTOR: summarization failed");
                    }
                    Err(e) => {
                        warn!(level, error = %e, "RAPTOR: task panicked");
                    }
                }
            }

            if next_level.is_empty() {
                break;
            }

            debug!(
                level,
                input_chunks = current_level.len(),
                summaries = next_level.len(),
                "RAPTOR: tree level built"
            );

            current_level = next_level;
            level += 1;
        }

        // Update token estimate
        let new_tokens: usize = enriched.chunks.iter()
            .map(|c| c.content.len() / 4) // rough estimate
            .sum();
        enriched.total_tokens_est = new_tokens;

        debug!(
            original_chunks = context.chunks.len(),
            total_chunks = enriched.chunks.len(),
            tree_depth = level,
            "RAPTOR: tree construction complete"
        );

        Ok(enriched)
    }
}

/// Split items into groups of at most `size`.
fn chunk_into_groups(items: &[String], size: usize) -> Vec<Vec<String>> {
    items.chunks(size).map(|c| c.to_vec()).collect()
}

const DEFAULT_RAPTOR: &str = "You are a hierarchical summarizer for a knowledge base. \
Given multiple text sections related to a query, {{abstraction}}\n\n\
Rules:\n\
- Preserve key facts, numbers, and named entities\n\
- Focus on information relevant to the query\n\
- Be concise but comprehensive\n\
- Do not add information not present in the sections";

/// Summarize a group of texts into a single summary.
async fn summarize_group(
    llm: &Arc<dyn LlmProvider>,
    query: &str,
    texts: &[String],
    level: u32,
    max_tokens: u32,
    prompts: &PromptRegistry,
) -> Result<String> {
    let combined: String = texts.iter().enumerate()
        .map(|(i, t)| format!("--- Section {} ---\n{}", i + 1, truncate(t, 1000)))
        .collect::<Vec<_>>()
        .join("\n\n");

    let abstraction = match level {
        0 => "Create a concise summary that captures the key facts and relationships.",
        1 => "Create a higher-level summary that synthesizes the main themes and conclusions.",
        _ => "Create a brief overview that captures the essential message.",
    };

    let system = ChatMessage {
        role: "system".into(),
        content: prompts.render_or_default("chat.raptor", DEFAULT_RAPTOR, &[("abstraction", abstraction)]),
    };

    let user = ChatMessage {
        role: "user".into(),
        content: format!("Query: {query}\n\nSections to summarize:\n{combined}"),
    };

    let resp = llm.generate(&[system, user], Some(max_tokens)).await?;
    Ok(resp.content.trim().to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { s[..max].to_string() }
}
