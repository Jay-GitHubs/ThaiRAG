use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, ChunkMetadata, DocumentAnalysis, DocumentChunk};
use tracing::{info, warn};

use super::analyzer::strip_json_fences;
use super::prompts;

/// Per-chunk output-token headroom for an enrichment batch. Each chunk yields a
/// JSON object with a context prefix, summary, bilingual keywords, and 2-3
/// hypothetical queries (~500-700 chars; more in token-heavy Thai). The flat
/// `agent_max_tokens` cap (often 1024) is sized for a single agent reply, not a
/// 5-chunk array, so it truncates the array mid-string and the whole batch is
/// lost. Scale the budget by batch size so the array can actually complete.
///
/// 400/chunk (2000 for a 5-batch) still truncated the trailing object on verbose
/// batches; 600 gives ~50% more headroom so the full array completes. Since this
/// is a cap, not a target, raising it costs nothing on batches that already fit —
/// the model stops when its JSON is done.
const PER_CHUNK_TOKEN_BUDGET: u32 = 600;

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

    /// Name of the model backing this agent.
    pub fn model_name(&self) -> &str {
        self.llm.model_name()
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

        let effective_max_tokens = self
            .max_tokens
            .max(batch.len() as u32 * PER_CHUNK_TOKEN_BUDGET);
        let response = self
            .llm
            .generate(&messages, Some(effective_max_tokens))
            .await?;
        let json_str = strip_json_fences(response.content.trim());

        match serde_json::from_str::<Vec<EnrichmentResult>>(json_str) {
            Ok(results) => Ok(results),
            Err(e) => {
                // The array may still be truncated (or trailing prose appended).
                // Salvage the complete leading objects so a partial response
                // still enriches the chunks that finished, instead of dropping
                // the whole batch.
                let salvaged = salvage_enrichment_objects(json_str);
                if salvaged.is_empty() {
                    Err(thairag_core::ThaiRagError::Internal(format!(
                        "Failed to parse enrichment response: {e}"
                    )))
                } else {
                    warn!(
                        recovered = salvaged.len(),
                        error = %e,
                        "Enrichment response not valid JSON; salvaged leading objects"
                    );
                    Ok(salvaged)
                }
            }
        }
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

/// Recover complete leading objects from a (possibly truncated) JSON array of
/// enrichment results. Scans for balanced top-level `{...}` objects — tracking
/// string literals and escapes so braces inside strings don't confuse depth —
/// and deserializes each one individually, stopping at the first object that
/// can't be parsed (e.g. the truncation point). Returns whatever parsed.
fn salvage_enrichment_objects(s: &str) -> Vec<EnrichmentResult> {
    let bytes = s.as_bytes();
    let mut results = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0
                    && let Some(st) = start.take()
                {
                    match serde_json::from_str::<EnrichmentResult>(&s[st..=i]) {
                        Ok(obj) => results.push(obj),
                        Err(_) => break,
                    }
                }
            }
            _ => {}
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salvages_complete_objects_from_truncated_array() {
        // Second object is cut off mid-string (the truncation the 1024-token
        // cap produced). The first complete object should still be recovered.
        let truncated = r#"[
            {"chunk_index": 0, "summary": "first", "keywords": ["a", "b"]},
            {"chunk_index": 1, "summary": "second is cut off mid-stri"#;
        let recovered = salvage_enrichment_objects(truncated);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].chunk_index, 0);
        assert_eq!(recovered[0].summary.as_deref(), Some("first"));
    }

    #[test]
    fn salvages_all_when_trailing_prose_appended() {
        let with_prose = r#"[
            {"chunk_index": 0, "summary": "one"},
            {"chunk_index": 2, "summary": "two"}
        ] Here is the explanation you asked me not to add."#;
        let recovered = salvage_enrichment_objects(with_prose);
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[1].chunk_index, 2);
    }

    #[test]
    fn ignores_braces_inside_strings() {
        let tricky = r#"[{"chunk_index": 0, "summary": "uses { and } braces", "context_prefix": "a \"quoted\" bit"}]"#;
        let recovered = salvage_enrichment_objects(tricky);
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            recovered[0].context_prefix.as_deref(),
            Some(r#"a "quoted" bit"#)
        );
    }

    #[test]
    fn returns_empty_when_nothing_complete() {
        let garbage = r#"[{"chunk_index": 0, "summary": "incompl"#;
        assert!(salvage_enrichment_objects(garbage).is_empty());
    }
}
