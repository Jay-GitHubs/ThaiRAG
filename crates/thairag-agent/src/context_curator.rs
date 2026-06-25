use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, ChunkId, DocId, ImageContent, ImageId, SearchResult};
use tracing::{debug, warn};

/// A curated chunk with relevance scoring and optional trimming.
#[derive(Debug, Clone)]
pub struct CuratedChunk {
    pub index: usize,
    pub content: String,
    pub relevance_score: f32,
    pub source_doc_id: DocId,
    pub source_chunk_id: ChunkId,
    /// Document title (resolved after curation for richer LLM context).
    pub source_doc_title: Option<String>,
    /// Source image blob (set when this chunk was derived from an image:
    /// PDF page render, embedded image, scanned page, or direct upload).
    /// Carried through from `ChunkMetadata.image_blob_id`.
    pub image_blob_id: Option<ImageId>,
    /// Hydrated image bytes for vision-capable answer LLMs. Empty until
    /// `hydrate_images` resolves `image_blob_id` against the store. Only
    /// populated when the answer LLM supports vision (PR-δ multimodal retrieval).
    pub images: Vec<ImageContent>,
}

/// Result of context curation.
#[derive(Debug, Clone, Default)]
pub struct CuratedContext {
    pub chunks: Vec<CuratedChunk>,
    pub total_tokens_est: usize,
}

impl CuratedContext {
    /// Populate `source_doc_title` on each chunk using the provided resolver.
    pub fn resolve_doc_titles(&mut self, resolver: &dyn Fn(DocId) -> Option<String>) {
        for chunk in &mut self.chunks {
            if chunk.source_doc_title.is_none() {
                chunk.source_doc_title = resolver(chunk.source_doc_id);
            }
        }
    }

    /// PR-δ multimodal retrieval: resolve each chunk's `image_blob_id` to image
    /// bytes via `resolver` and stash them on the chunk for the answer LLM's
    /// vision input. Stops after `max_images` total images to bound the request
    /// payload (vision blobs are large). Callers MUST gate on the answer LLM's
    /// `supports_vision()` — this method does not check.
    pub fn hydrate_images(
        &mut self,
        resolver: &dyn Fn(ImageId) -> Option<ImageContent>,
        max_images: usize,
    ) {
        let mut budget = max_images;
        for chunk in &mut self.chunks {
            if budget == 0 {
                break;
            }
            if let Some(img_id) = chunk.image_blob_id
                && chunk.images.is_empty()
                && let Some(img) = resolver(img_id)
            {
                chunk.images.push(img);
                budget -= 1;
            }
        }
    }
}

/// JSON schema mirroring [`LlmCuration`] for grammar-constrained decoding.
fn curation_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "selected": {"type": "array", "items": {"type": "integer", "minimum": 1}}
        },
        "required": ["selected"]
    })
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
                - Stay within the token budget (estimate ~4 chars per token for English, ~1.5 for Thai)\n\
                Output ONLY valid JSON.";

pub struct ContextCurator {
    llm: Arc<dyn LlmProvider>,
    max_context_tokens: usize,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl ContextCurator {
    pub fn new(llm: Arc<dyn LlmProvider>, max_context_tokens: usize, max_tokens: u32) -> Self {
        Self {
            llm,
            max_context_tokens,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_context_tokens: usize,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_context_tokens,
            max_tokens,
            prompts,
        }
    }

    pub async fn curate(
        &self,
        query: &str,
        results: &[SearchResult],
        image_budget: ImageBudget,
    ) -> Result<CuratedContext> {
        if results.is_empty() {
            return Ok(CuratedContext {
                chunks: vec![],
                total_tokens_est: 0,
            });
        }

        // Build chunk list for LLM
        let chunk_list: String = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let preview: String = r.chunk.content.chars().take(300).collect();
                format!("[{}] (score: {:.2}) {}", i + 1, r.score, preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let max_ctx = self.max_context_tokens.to_string();
        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.context_curator",
                DEFAULT_TEMPLATE,
                &[("max_context_tokens", &max_ctx)],
            ),
            images: vec![],
        };
        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}\n\nChunks:\n{chunk_list}"),
            images: vec![],
        };

        let selected_indices = match self
            .llm
            .generate_structured(&[system, user], Some(self.max_tokens), &curation_schema())
            .await
        {
            Ok(resp) => {
                let json_str = thairag_core::extract_json(resp.content.trim());
                match serde_json::from_str::<LlmCuration>(json_str) {
                    Ok(c) => {
                        debug!(selected = c.selected.len(), "Chunks curated by LLM");
                        c.selected
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse LLM curation, using all chunks");
                        crate::degradation::record_fallback("context_curator");
                        (1..=results.len()).collect()
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM curation failed, using all chunks");
                crate::degradation::record_fallback("context_curator");
                (1..=results.len()).collect()
            }
        };

        // Safety net: search + rerank already filtered to relevant results, so a
        // curator that selects nothing would answer from empty context — almost
        // always wrong (the answer LLM, not the curator, is responsible for
        // saying "not found"). Small curator models do emit a valid `[]`, so
        // never let an empty selection collapse a non-empty result set.
        let selected_indices = if selected_indices.is_empty() {
            warn!(
                retrieved = results.len(),
                "LLM curation selected no chunks; keeping all to avoid empty context"
            );
            (1..=results.len()).collect()
        } else {
            selected_indices
        };

        build_curated_context(
            results,
            &selected_indices,
            self.max_context_tokens,
            image_budget,
        )
    }
}

/// Estimate LLM token count for context-budget enforcement.
/// (rough: 4 chars/token English, 1.5 chars/token Thai).
///
/// Thai calibration — `~1.5 chars/token` for Qwen. Production Thai documents
/// (legal text, gazettes, tables, digits) tokenize FAR worse than clean prose:
/// rare characters, numerals and mixed scripts fall to near byte-level in the
/// Qwen BPE tokenizer, so real `prompt_eval_count` runs ~1.3–1.6 chars/token —
/// not the ~2.0 measured on clean prose. We deliberately use the conservative
/// 1.5 because the failure modes are asymmetric: **under-counting overflows the
/// model context window (silent truncation / OOM)** on Thai-heavy prompts —
/// observed as ~16.7K real tokens against a budget that thought it was ~6K —
/// which is worse than over-counting (dropping one chunk at the boundary).
///
/// History: an early version OVER-counted Thai ~2× (subtracting the Thai CHAR
/// count from the BYTE length); a 2026-06 pass swung to 2.5 chars/token, which
/// then UNDER-counted real documents and overflowed 16K contexts. 1.5 is the
/// corrected, overflow-safe value for Qwen on production Thai.
/// Configured Thai chars/token calibration, stored ×1000 (1500 == 1.5).
/// Process-global: the ratio depends only on the model's tokenizer, not the
/// workspace/scope, so a single per-deployment value is correct. Set once from
/// `chat_pipeline.thai_chars_per_token` when the pipeline is built; defaults to
/// 1.5 if never set (e.g. in unit tests that don't construct a pipeline).
static THAI_CHARS_PER_TOKEN_MILLI: AtomicU32 = AtomicU32::new(1500);

/// Override the Thai chars/token calibration from config (see
/// [`estimate_tokens`]). Non-finite or out-of-range values are clamped to
/// `[0.5, 4.0]`; `<= 0` falls back to the 1.5 default. Idempotent and cheap —
/// safe to call on every pipeline build.
pub fn set_thai_chars_per_token(ratio: f32) {
    let clamped = if ratio.is_finite() && ratio > 0.0 {
        ratio.clamp(0.5, 4.0)
    } else {
        1.5
    };
    THAI_CHARS_PER_TOKEN_MILLI.store((clamped * 1000.0).round() as u32, Ordering::Relaxed);
}

fn thai_chars_per_token() -> f32 {
    THAI_CHARS_PER_TOKEN_MILLI.load(Ordering::Relaxed) as f32 / 1000.0
}

fn estimate_tokens(text: &str) -> usize {
    let mut thai_chars = 0usize;
    let mut other_chars = 0usize;
    for c in text.chars() {
        if ('\u{0E01}'..='\u{0E5B}').contains(&c) {
            thai_chars += 1;
        } else {
            other_chars += 1;
        }
    }
    // Thai cost is the configured chars/token (default 1.5); other ≈ 4 chars/token.
    let thai_tokens = (thai_chars as f32 / thai_chars_per_token()).ceil() as usize;
    thai_tokens + other_chars / 4 + 1
}

/// Per-request image-token reservation for context-budget accounting.
///
/// When the answer LLM is vision-capable, each image-bearing chunk that will be
/// hydrated into the prompt costs `tokens_per_image` against the budget (capped
/// at `max_images`, matching the hydration cap), so an image-heavy multimodal
/// prompt can't silently overflow the model context window. `none()` (both 0)
/// for text-only answer paths — images are never sent, so they reserve nothing.
#[derive(Clone, Copy)]
pub struct ImageBudget {
    pub tokens_per_image: usize,
    pub max_images: usize,
}

impl ImageBudget {
    /// No image accounting — text-only answer path.
    pub fn none() -> Self {
        Self {
            tokens_per_image: 0,
            max_images: 0,
        }
    }
}

fn build_curated_context(
    results: &[SearchResult],
    selected: &[usize],
    max_tokens: usize,
    image_budget: ImageBudget,
) -> Result<CuratedContext> {
    let mut chunks = Vec::new();
    let mut total_tokens = 0;
    let mut images_reserved = 0usize;

    for (rank, &idx) in selected.iter().enumerate() {
        let i = idx.saturating_sub(1); // Convert 1-based to 0-based
        if i >= results.len() {
            continue;
        }

        let r = &results[i];
        let image_blob_id = r.chunk.metadata.as_ref().and_then(|m| m.image_blob_id);
        // Charge the per-image cost only for chunks whose image will actually be
        // hydrated: vision answer path (tokens_per_image > 0), this chunk has an
        // image, and we're under the hydration cap. Matches hydrate_images.
        let image_cost = if image_budget.tokens_per_image > 0
            && image_blob_id.is_some()
            && images_reserved < image_budget.max_images
        {
            image_budget.tokens_per_image
        } else {
            0
        };
        let tokens = estimate_tokens(&r.chunk.content) + image_cost;

        if total_tokens + tokens > max_tokens && !chunks.is_empty() {
            break; // Hit budget (text + reserved image tokens)
        }

        if image_cost > 0 {
            images_reserved += 1;
        }
        chunks.push(CuratedChunk {
            index: rank + 1,
            content: r.chunk.content.clone(),
            relevance_score: r.score,
            source_doc_id: r.chunk.doc_id,
            source_chunk_id: r.chunk.chunk_id,
            source_doc_title: None,
            image_blob_id,
            images: Vec::new(),
        });
        total_tokens += tokens;
    }

    Ok(CuratedContext {
        chunks,
        total_tokens_est: total_tokens,
    })
}

/// Fallback: take top-K chunks directly without LLM curation.
pub fn fallback_curate(
    results: &[SearchResult],
    max_tokens: usize,
    image_budget: ImageBudget,
) -> CuratedContext {
    let indices: Vec<usize> = (1..=results.len()).collect();
    build_curated_context(results, &indices, max_tokens, image_budget).unwrap_or(CuratedContext {
        chunks: vec![],
        total_tokens_est: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{ChunkMetadata, DocumentChunk, WorkspaceId};

    fn result_with_image(content: &str, image_blob_id: Option<ImageId>) -> SearchResult {
        SearchResult {
            chunk: DocumentChunk {
                chunk_id: ChunkId::new(),
                doc_id: DocId::new(),
                workspace_id: WorkspaceId::new(),
                content: content.into(),
                chunk_index: 0,
                embedding: None,
                metadata: image_blob_id.map(|id| ChunkMetadata {
                    image_blob_id: Some(id),
                    ..Default::default()
                }),
            },
            score: 0.9,
        }
    }

    fn dummy_image() -> ImageContent {
        ImageContent {
            base64_data: "AAAA".into(),
            media_type: "image/png".into(),
        }
    }

    // Serializes the tests that mutate the process-global Thai calibration so
    // they don't race under the parallel test runner.
    static EST_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn token_estimate_calibrated_for_thai() {
        let _g = EST_TEST_LOCK.lock().unwrap();
        set_thai_chars_per_token(1.5); // pin the default for a deterministic read
        // 100 Thai chars ≈ 67 tokens (conservative 1.5 chars/token) — Qwen
        // tokenizes production Thai near byte-level, so the budget must assume
        // the worst case to avoid overflowing the context window.
        let thai: String = "ธ".repeat(100);
        let est = estimate_tokens(&thai);
        assert!((66..=70).contains(&est), "thai estimate {est}");
        // 100 ASCII chars ≈ 25 tokens (4 chars/token, unchanged).
        let en = "a".repeat(100);
        let est = estimate_tokens(&en);
        assert!((24..=27).contains(&est), "english estimate {est}");
        // Thai must estimate clearly MORE tokens than the same char count of
        // English — the asymmetry the old 2.5 ratio under-counted.
        assert!(
            estimate_tokens(&thai) > estimate_tokens(&en) * 2,
            "thai should cost >2x english per char"
        );
    }

    #[test]
    fn thai_chars_per_token_is_configurable() {
        let _g = EST_TEST_LOCK.lock().unwrap();
        let thai: String = "ธ".repeat(100);
        // More aggressive (1.0 chars/token) → MORE estimated tokens.
        set_thai_chars_per_token(1.0);
        assert!((100..=102).contains(&estimate_tokens(&thai)), "ratio 1.0");
        // Looser (2.0) → FEWER estimated tokens.
        set_thai_chars_per_token(2.0);
        assert!((50..=52).contains(&estimate_tokens(&thai)), "ratio 2.0");
        // Garbage / out-of-range inputs clamp safely (never 0 or NaN).
        set_thai_chars_per_token(0.0); // → falls back to 1.5
        assert!(
            (66..=70).contains(&estimate_tokens(&thai)),
            "ratio 0 → default"
        );
        set_thai_chars_per_token(99.0); // → clamps to 4.0
        let est = estimate_tokens(&thai);
        assert!((24..=27).contains(&est), "ratio 99 clamps to 4.0: {est}");
        set_thai_chars_per_token(1.5); // restore default for other tests
    }

    #[test]
    fn build_curated_context_carries_image_blob_id() {
        let img = ImageId::new();
        let results = [
            result_with_image("text chunk", None),
            result_with_image("image chunk", Some(img)),
        ];
        let ctx = fallback_curate(&results, 10_000, ImageBudget::none());
        assert_eq!(ctx.chunks.len(), 2);
        assert_eq!(ctx.chunks[0].image_blob_id, None);
        assert_eq!(ctx.chunks[1].image_blob_id, Some(img));
        // Nothing is hydrated until hydrate_images runs.
        assert!(ctx.chunks.iter().all(|c| c.images.is_empty()));
    }

    #[test]
    fn image_budget_reserves_tokens_for_hydrated_images() {
        let results = [
            result_with_image("a", None),
            result_with_image("b", Some(ImageId::new())),
            result_with_image("c", Some(ImageId::new())),
        ];
        // Text-only path: tiny budget still admits all three (chars ≈ 1 token each).
        let text_only = fallback_curate(&results, 100, ImageBudget::none());
        assert_eq!(text_only.chunks.len(), 3);

        // Vision path: each image-bearing chunk now costs +1500 tokens, so a
        // 2000-token budget fits the text chunk + exactly ONE image chunk
        // (1 + 1500 + ... the 2nd image would blow 2000).
        let img = ImageBudget {
            tokens_per_image: 1500,
            max_images: 4,
        };
        let vision = fallback_curate(&results, 2000, img);
        assert_eq!(vision.chunks.len(), 2, "text + 1 image fits 2000");
        // The reported estimate includes the reserved image tokens.
        assert!(
            vision.total_tokens_est >= 1500,
            "image tokens counted: {}",
            vision.total_tokens_est
        );

        // The hydration cap bounds the reservation: with max_images=1 only the
        // first image is charged, so a later image chunk is admitted text-only.
        let capped = ImageBudget {
            tokens_per_image: 1500,
            max_images: 1,
        };
        let ctx = fallback_curate(&results, 4000, capped);
        assert_eq!(ctx.chunks.len(), 3, "cap charges only the first image");
    }

    #[test]
    fn hydrate_images_resolves_only_image_chunks() {
        let img = ImageId::new();
        let results = [
            result_with_image("text chunk", None),
            result_with_image("image chunk", Some(img)),
        ];
        let mut ctx = fallback_curate(&results, 10_000, ImageBudget::none());
        ctx.hydrate_images(&|_id| Some(dummy_image()), 10);
        assert!(ctx.chunks[0].images.is_empty(), "text chunk stays empty");
        assert_eq!(ctx.chunks[1].images.len(), 1, "image chunk hydrated");
    }

    #[test]
    fn hydrate_images_honors_max_cap() {
        let results: Vec<SearchResult> = (0..5)
            .map(|i| result_with_image(&format!("img {i}"), Some(ImageId::new())))
            .collect();
        let mut ctx = fallback_curate(&results, 100_000, ImageBudget::none());
        ctx.hydrate_images(&|_id| Some(dummy_image()), 2);
        let hydrated = ctx.chunks.iter().filter(|c| !c.images.is_empty()).count();
        assert_eq!(hydrated, 2, "cap limits total hydrated images");
    }

    #[test]
    fn hydrate_images_skips_when_resolver_returns_none() {
        let results = [result_with_image("image chunk", Some(ImageId::new()))];
        let mut ctx = fallback_curate(&results, 10_000, ImageBudget::none());
        ctx.hydrate_images(&|_id| None, 10);
        assert!(ctx.chunks[0].images.is_empty());
    }

    /// LLM stub that returns a fixed curation reply.
    struct StubCurator {
        reply: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for StubCurator {
        fn model_name(&self) -> &str {
            "stub-curator"
        }
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<thairag_core::types::LlmResponse> {
            Ok(thairag_core::types::LlmResponse {
                content: self.reply.clone(),
                usage: Default::default(),
            })
        }
        async fn generate_vision(
            &self,
            _messages: &[thairag_core::types::VisionMessage],
            _max_tokens: Option<u32>,
        ) -> Result<thairag_core::types::LlmResponse> {
            unreachable!("curator does not use vision")
        }
    }

    #[tokio::test]
    async fn empty_curation_falls_back_to_all_retrieved_chunks() {
        // A small curator model can emit a valid `{"selected": []}`. Search +
        // rerank already vetted these results, so the curator must not collapse
        // them to empty context (which would force an "I have no information"
        // answer despite a relevant chunk being retrieved).
        let llm = Arc::new(StubCurator {
            reply: "{\"selected\": []}".into(),
        });
        let curator = ContextCurator::new(llm, 10_000, 256);
        let results = [
            result_with_image("North | 100 | 200", None),
            result_with_image("Northeast | 1100 | 1200", None),
        ];
        let ctx = curator
            .curate("Northeast Q1 sales", &results, ImageBudget::none())
            .await
            .unwrap();
        assert_eq!(ctx.chunks.len(), 2, "empty selection must keep all chunks");
        assert!(ctx.chunks.iter().any(|c| c.content.contains("1100")));
    }
}
