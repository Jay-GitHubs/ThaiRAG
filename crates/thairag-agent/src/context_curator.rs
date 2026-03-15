use std::sync::Arc;

use serde::Deserialize;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, ChunkId, DocId, SearchResult};
use thairag_core::PromptRegistry;
use tracing::{debug, warn};

/// A curated chunk with relevance scoring and optional trimming.
#[derive(Debug, Clone)]
pub struct CuratedChunk {
    pub index: usize,
    pub content: String,
    pub relevance_score: f32,
    pub source_doc_id: DocId,
    pub source_chunk_id: ChunkId,
}

/// Result of context curation.
#[derive(Debug, Clone, Default)]
pub struct CuratedContext {
    pub chunks: Vec<CuratedChunk>,
    pub total_tokens_est: usize,
}

#[derive(Deserialize)]
struct LlmCuration {
    /// Indices of relevant chunks (1-based), ordered by relevance.
    #[serde(default)]
    selected: Vec<usize>,
}

const DEFAULT_TEMPLATE: &str = "You are a context curator. Given a user query and retrieved chunks, \
                select the most relevant chunks and order them by relevance.\n\n\
                Budget: ~{{max_context_tokens}} tokens of context.\n\n\
                Output JSON only:\n\
                {\"selected\":[1,3,2]}\n\n\
                Rules:\n\
                - List chunk numbers (1-based) in order of relevance\n\
                - Exclude chunks that are irrelevant to the query\n\
                - Stay within the token budget (estimate ~4 chars per token for English, ~2 for Thai)\n\
                Output ONLY valid JSON.";

pub struct ContextCurator {
    llm: Arc<dyn LlmProvider>,
    max_context_tokens: usize,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl ContextCurator {
    pub fn new(llm: Arc<dyn LlmProvider>, max_context_tokens: usize, max_tokens: u32) -> Self {
        Self { llm, max_context_tokens, max_tokens, prompts: Arc::new(PromptRegistry::new()) }
    }

    pub fn new_with_prompts(llm: Arc<dyn LlmProvider>, max_context_tokens: usize, max_tokens: u32, prompts: Arc<PromptRegistry>) -> Self {
        Self { llm, max_context_tokens, max_tokens, prompts }
    }

    pub async fn curate(
        &self,
        query: &str,
        results: &[SearchResult],
    ) -> Result<CuratedContext> {
        if results.is_empty() {
            return Ok(CuratedContext { chunks: vec![], total_tokens_est: 0 });
        }

        // Build chunk list for LLM
        let chunk_list: String = results.iter().enumerate().map(|(i, r)| {
            let preview: String = r.chunk.content.chars().take(300).collect();
            format!("[{}] (score: {:.2}) {}", i + 1, r.score, preview)
        }).collect::<Vec<_>>().join("\n");

        let max_ctx = self.max_context_tokens.to_string();
        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.context_curator",
                DEFAULT_TEMPLATE,
                &[("max_context_tokens", &max_ctx)],
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}\n\nChunks:\n{chunk_list}"),
        };

        let selected_indices = match self.llm.generate(&[system, user], Some(self.max_tokens)).await {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<LlmCuration>(json_str) {
                    Ok(c) => {
                        debug!(selected = c.selected.len(), "Chunks curated by LLM");
                        c.selected
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse LLM curation, using all chunks");
                        (1..=results.len()).collect()
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM curation failed, using all chunks");
                (1..=results.len()).collect()
            }
        };

        build_curated_context(results, &selected_indices, self.max_context_tokens)
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

/// Estimate token count (rough: 4 chars/token English, 2 chars/token Thai).
fn estimate_tokens(text: &str) -> usize {
    let thai_chars = text.chars().filter(|c| ('\u{0E01}'..='\u{0E5B}').contains(c)).count();
    let other_chars = text.len() - thai_chars;
    (thai_chars / 2) + (other_chars / 4) + 1
}

fn build_curated_context(
    results: &[SearchResult],
    selected: &[usize],
    max_tokens: usize,
) -> Result<CuratedContext> {
    let mut chunks = Vec::new();
    let mut total_tokens = 0;

    for (rank, &idx) in selected.iter().enumerate() {
        let i = idx.saturating_sub(1); // Convert 1-based to 0-based
        if i >= results.len() { continue; }

        let r = &results[i];
        let tokens = estimate_tokens(&r.chunk.content);

        if total_tokens + tokens > max_tokens && !chunks.is_empty() {
            break; // Hit budget
        }

        chunks.push(CuratedChunk {
            index: rank + 1,
            content: r.chunk.content.clone(),
            relevance_score: r.score,
            source_doc_id: r.chunk.doc_id,
            source_chunk_id: r.chunk.chunk_id,
        });
        total_tokens += tokens;
    }

    Ok(CuratedContext { chunks, total_tokens_est: total_tokens })
}

/// Fallback: take top-K chunks directly without LLM curation.
pub fn fallback_curate(results: &[SearchResult], max_tokens: usize) -> CuratedContext {
    let indices: Vec<usize> = (1..=results.len()).collect();
    build_curated_context(results, &indices, max_tokens).unwrap_or(CuratedContext {
        chunks: vec![],
        total_tokens_est: 0,
    })
}
