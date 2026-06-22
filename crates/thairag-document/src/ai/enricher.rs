use std::sync::Arc;

use serde::{Deserialize, Deserializer};
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
/// multi-chunk array, so it truncates the array mid-string and the whole batch
/// is lost. Scale the budget by batch size so the array can actually complete.
///
/// At 400/chunk a 5-chunk array still truncated its trailing object; 600 plus a
/// smaller [`BATCH_SIZE`] gives enough headroom for the array to complete. Since
/// this is a cap, not a target, raising it costs nothing on batches that already
/// fit — the model stops when its JSON is done.
const PER_CHUNK_TOKEN_BUDGET: u32 = 600;

/// Chunks per enrichment LLM call. Smaller arrays complete more reliably (a
/// verbose object is far less likely to overrun the token budget mid-array), at
/// the cost of more calls. Dropped from 5 → 3 to reach full chunk coverage.
const BATCH_SIZE: usize = 3;

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
    #[serde(deserialize_with = "deserialize_chunk_index")]
    chunk_index: usize,
    context_prefix: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    hypothetical_queries: Option<Vec<String>>,
}

/// Deserialize `chunk_index`, tolerating the quoted-number form some VL models
/// emit despite the integer schema (e.g. qwen2.5-vl-7b returns `"chunk_index":
/// "6"`). serde's default `usize` decoder rejects the string with `invalid type:
/// string "6", expected usize` and the whole enrichment batch is dropped. Accept
/// both an integer and a numeric string so a schema-loose model doesn't cost us
/// the batch.
fn deserialize_chunk_index<'de, D>(deserializer: D) -> std::result::Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntOrString {
        Int(usize),
        Str(String),
    }

    match IntOrString::deserialize(deserializer)? {
        IntOrString::Int(n) => Ok(n),
        IntOrString::Str(s) => s
            .trim()
            .parse::<usize>()
            .map_err(|_| serde::de::Error::custom(format!("invalid chunk_index string: {s:?}"))),
    }
}

impl LlmChunkEnricher {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            batch_size: BATCH_SIZE,
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
            batch_size: BATCH_SIZE,
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
            .generate_structured(&messages, Some(effective_max_tokens), &enrichment_schema())
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

/// JSON schema for one enrichment batch: an array of result objects with hard
/// per-field bounds. Passed to the LLM provider's schema-enforced decoding
/// (Ollama `format`) so the model can only emit conforming JSON — it cannot
/// ramble into unbounded prose that overruns the token cap and truncates the
/// array mid-string. The `maxLength`/`maxItems` bounds mirror the prompt's
/// stated limits, turning "please be terse" into a decoder-level guarantee.
/// Providers without schema support ignore this and fall back to plain text.
fn enrichment_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "chunk_index": { "type": "integer" },
                "context_prefix": { "type": "string", "maxLength": 120 },
                "summary": { "type": "string", "maxLength": 220 },
                "keywords": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 40 },
                    "maxItems": 5
                },
                "hypothetical_queries": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 140 },
                    "maxItems": 2
                }
            },
            "required": ["chunk_index", "summary"]
        }
    })
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

    #[test]
    fn parses_quoted_chunk_index_from_vl_models() {
        // qwen2.5-vl-7b emits chunk_index as a quoted string despite the integer
        // schema. Strict usize decoding dropped the whole batch with
        // `invalid type: string "6", expected usize`; we now accept both forms.
        let quoted = r#"[
            {"chunk_index": "6", "summary": "six"},
            {"chunk_index": 7, "summary": "seven"}
        ]"#;
        let parsed: Vec<EnrichmentResult> = serde_json::from_str(quoted).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].chunk_index, 6);
        assert_eq!(parsed[1].chunk_index, 7);

        // Salvage path must tolerate it too (partial batches from VL models).
        let salvaged = salvage_enrichment_objects(quoted);
        assert_eq!(salvaged.len(), 2);
        assert_eq!(salvaged[0].chunk_index, 6);
    }
}
