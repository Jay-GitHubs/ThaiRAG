use std::sync::Arc;

use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmUsage, SearchResult};
use tracing::{debug, info, warn};

use crate::query_analyzer::{Complexity, QueryAnalysis};

/// Mapped output from a single chunk.
#[derive(Debug, Clone)]
pub struct MappedChunk {
    pub chunk_content: String,
    pub extracted_info: String,
    pub source_index: usize,
}

/// Map-Reduce RAG: processes large numbers of document chunks by mapping each
/// one independently (extracting relevant info) then reducing all partial answers
/// into a single coherent response. Ideal for synthesis, comparison, and analysis
/// queries that span multiple documents.
pub struct MapReduceRag {
    llm: Arc<dyn LlmProvider>,
    max_chunks: usize,
    map_max_tokens: u32,
    reduce_max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl MapReduceRag {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        max_chunks: usize,
        map_max_tokens: u32,
        reduce_max_tokens: u32,
    ) -> Self {
        Self {
            llm,
            max_chunks,
            map_max_tokens,
            reduce_max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_chunks: usize,
        map_max_tokens: u32,
        reduce_max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_chunks,
            map_max_tokens,
            reduce_max_tokens,
            prompts,
        }
    }

    /// Decide whether map-reduce is appropriate for this query.
    pub fn should_use(&self, analysis: &QueryAnalysis, results: &[SearchResult]) -> bool {
        // Use map-reduce for complex queries with many results
        let many_results = results.len() >= 8;
        let is_complex = analysis.complexity == Complexity::Complex;

        is_complex && many_results
    }

    /// MAP phase: process each chunk independently, extracting info relevant to the query.
    /// Runs in parallel for speed.
    pub async fn map_phase(
        &self,
        query: &str,
        results: &[SearchResult],
    ) -> Result<Vec<MappedChunk>> {
        let chunks_to_process = results.iter().take(self.max_chunks);

        const DEFAULT_MAP_PROMPT: &str = r#"Extract ONLY the information from the provided text that is relevant to answering the query.
If the text contains no relevant information, respond with "NO_RELEVANT_INFO".
Be factual and concise. Preserve key data points, names, dates, and numbers."#;

        let mut handles = Vec::new();
        for (idx, result) in chunks_to_process.enumerate() {
            let llm = Arc::clone(&self.llm);
            let prompts = Arc::clone(&self.prompts);
            let query = query.to_string();
            let content = result.chunk.content.clone();
            let max_tokens = self.map_max_tokens;

            handles.push(tokio::spawn(async move {
                let system = ChatMessage {
                    role: "system".into(),
                    content: prompts.render_or_default(
                        "chat.map_reduce_map",
                        DEFAULT_MAP_PROMPT,
                        &[],
                    ),
                    images: vec![],
                };

                let user = ChatMessage {
                    role: "user".into(),
                    content: format!("Query: {query}\n\nDocument chunk [{idx}]:\n{content}"),
                    images: vec![],
                };

                match llm.generate(&[system, user], Some(max_tokens)).await {
                    Ok(resp) => {
                        let extracted = resp.content.trim().to_string();
                        if extracted.contains("NO_RELEVANT_INFO") {
                            None
                        } else {
                            Some(MappedChunk {
                                chunk_content: content,
                                extracted_info: extracted,
                                source_index: idx,
                            })
                        }
                    }
                    Err(e) => {
                        warn!(idx, error = %e, "Map-Reduce: map phase failed for chunk");
                        None
                    }
                }
            }));
        }

        let mut mapped = Vec::new();
        for handle in handles {
            if let Ok(Some(chunk)) = handle.await {
                mapped.push(chunk);
            }
        }

        debug!(
            total = results.len(),
            mapped = mapped.len(),
            "Map-Reduce: map phase complete"
        );
        Ok(mapped)
    }

    /// REDUCE phase: synthesize all mapped extractions into a single coherent answer.
    pub async fn reduce_phase(&self, query: &str, mapped: &[MappedChunk]) -> Result<LlmResponse> {
        if mapped.is_empty() {
            return Ok(LlmResponse {
                content: "I found relevant documents but couldn't extract information pertinent to your query.".into(),
                usage: LlmUsage::default(),
            });
        }

        let extractions: String = mapped
            .iter()
            .map(|m| format!("[Source {}]: {}", m.source_index + 1, m.extracted_info))
            .collect::<Vec<_>>()
            .join("\n\n");

        const DEFAULT_REDUCE_PROMPT: &str = r#"Synthesize the extracted information from multiple document sources into a single, coherent answer.

Guidelines:
- Combine related facts from different sources
- Resolve any contradictions by noting them
- Use citations [1][2] to reference source numbers
- Organize the answer logically (chronologically, by theme, or by importance)
- Be comprehensive but avoid repetition"#;

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.map_reduce_reduce",
                DEFAULT_REDUCE_PROMPT,
                &[],
            ),
            images: vec![],
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Query: {query}\n\nExtracted information from {n} sources:\n\n{extractions}",
                n = mapped.len()
            ),
            images: vec![],
        };

        match self
            .llm
            .generate(&[system, user], Some(self.reduce_max_tokens))
            .await
        {
            Ok(resp) => {
                info!(
                    sources = mapped.len(),
                    response_len = resp.content.len(),
                    "Map-Reduce: reduce phase complete"
                );
                Ok(resp)
            }
            Err(e) => {
                warn!(error = %e, "Map-Reduce: reduce phase failed");
                // Fallback: concatenate extractions
                let fallback = mapped
                    .iter()
                    .map(|m| format!("[{}] {}", m.source_index + 1, m.extracted_info))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Ok(LlmResponse {
                    content: fallback,
                    usage: LlmUsage::default(),
                })
            }
        }
    }

    /// Full map-reduce pipeline: map then reduce.
    pub async fn process(&self, query: &str, results: &[SearchResult]) -> Result<LlmResponse> {
        let mapped = self.map_phase(query, results).await?;
        self.reduce_phase(query, &mapped).await
    }
}
