use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, ChunkMetadata, DocumentAnalysis, DocumentChunk};
use tracing::{info, warn};

use super::analyzer::strip_json_fences;
use super::prompts;

/// LLM-powered chunk enricher.
///
/// Processes chunks in batches to generate search-optimized metadata:
/// context prefix, summary, keywords, and hypothetical queries (HyDE).
pub struct LlmChunkEnricher {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    /// Number of chunks to process per LLM call.
    batch_size: usize,
    prompts: Arc<PromptRegistry>,
}

#[derive(Debug, Deserialize)]
struct EnrichmentResult {
    chunk_index: usize,
    context_prefix: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    hypothetical_queries: Option<Vec<String>>,
}

impl LlmChunkEnricher {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            batch_size: 5,
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
            batch_size: 5,
            prompts,
        }
    }

    /// Enrich chunks with search-optimized metadata.
    /// Modifies chunks in-place, adding context_prefix, summary, keywords,
    /// hypothetical_queries, and prepending/appending content for better embeddings.
    pub async fn enrich(
        &self,
        chunks: &mut [DocumentChunk],
        analysis: &DocumentAnalysis,
        document_title: &str,
    ) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let content_type = format!("{:?}", analysis.content_type);
        let total = chunks.len();
        let mut enriched_count = 0usize;

        // Process in batches
        for batch_start in (0..total).step_by(self.batch_size) {
            let batch_end = (batch_start + self.batch_size).min(total);
            let batch_items: Vec<(usize, &str)> = (batch_start..batch_end)
                .map(|i| (i, chunks[i].content.as_str()))
                .collect();

            match self
                .enrich_batch(
                    &batch_items,
                    document_title,
                    &analysis.primary_language,
                    &content_type,
                )
                .await
            {
                Ok(results) => {
                    for result in results {
                        if result.chunk_index >= total {
                            continue;
                        }
                        let chunk = &mut chunks[result.chunk_index];
                        self.apply_enrichment(chunk, result);
                        enriched_count += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        batch_start, batch_end,
                        error = %e,
                        "Chunk enrichment batch failed, skipping batch"
                    );
                }
            }
        }

        info!(
            enriched = enriched_count,
            total, "Chunk enrichment complete"
        );
        Ok(())
    }

    async fn enrich_batch(
        &self,
        batch: &[(usize, &str)],
        document_title: &str,
        primary_language: &str,
        content_type: &str,
    ) -> Result<Vec<EnrichmentResult>> {
        let prompt = prompts::chunk_enricher_prompt(
            &self.prompts,
            batch,
            document_title,
            primary_language,
            content_type,
        );
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt,
            images: vec![],
        }];

        let response = self.llm.generate(&messages, Some(self.max_tokens)).await?;
        let json_str = strip_json_fences(response.content.trim());

        let results: Vec<EnrichmentResult> = serde_json::from_str(json_str).map_err(|e| {
            thairag_core::ThaiRagError::Internal(format!(
                "Failed to parse enrichment response: {e}"
            ))
        })?;

        Ok(results)
    }

    fn apply_enrichment(&self, chunk: &mut DocumentChunk, result: EnrichmentResult) {
        // Store original content before modification
        let original_content = chunk.content.clone();

        // Build enriched content: context prefix + original + HyDE queries
        let mut enriched = String::new();

        if let Some(ref prefix) = result.context_prefix {
            enriched.push_str(prefix);
            enriched.push('\n');
        }

        enriched.push_str(&original_content);

        if let Some(ref queries) = result.hypothetical_queries
            && !queries.is_empty()
        {
            enriched.push_str("\n\n[Related questions: ");
            enriched.push_str(&queries.join(" | "));
            enriched.push(']');
        }

        chunk.content = enriched;

        // Update metadata
        let metadata = chunk.metadata.get_or_insert_with(ChunkMetadata::default);
        metadata.context_prefix = result.context_prefix;
        metadata.summary = result.summary;
        metadata.keywords = result.keywords;
        metadata.hypothetical_queries = result.hypothetical_queries;
        metadata.original_content = Some(original_content);
    }
}
