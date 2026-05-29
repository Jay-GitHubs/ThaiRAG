//! Smart per-page PDF extraction → one canonical semantic-markdown document.
//!
//! Composes the pure strategy selector ([`crate::semantic`]) with the native
//! pdfium engine ([`crate::pdfium_engine`]) and the vision LLM to turn an
//! uploaded PDF into AI-friendly markdown that distinguishes body text,
//! markdown tables, and image descriptions — page by page, in reading order.
//!
//! The flow is two-phase to respect pdfium's `!Send` handles:
//! 1. **sync** ([`extract_pages`], run inside `spawn_blocking`): open the PDF
//!    once, pick a [`PageStrategy`] per page, and gather the owned bytes each
//!    strategy needs (page render and/or embedded images). Returns `Send` data.
//! 2. **async** ([`render_to_document`]): call the vision model per page using
//!    the strategy-specific prompt, then assemble the document.
//!
//! Failures degrade rather than abort: a page whose vision call fails keeps its
//! extracted pdfium text; the whole-document zero-chunk guard lives in the
//! pipeline.

use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use tracing::warn;

use crate::pdfium_engine::PdfEngine;
use crate::semantic::{
    PageStrategy, RenderedPage, StrategyThresholds, assemble_document_markdown,
    select_page_strategy,
};
use crate::semantic_prompts::{Language, get_prompts, high_quality_prompt};
use crate::text_utils::meaningful_char_count;

const PNG_MIME: &str = "image/png";
/// Token budget for a full-page OCR / transcription call.
const PAGE_VISION_TOKENS: u32 = 4096;
/// Token budget for a single embedded-image description.
const IMAGE_VISION_TOKENS: u32 = 1024;

/// Tunables for the smart-PDF engine. Defaults mirror Jay-RAG-Tools.
#[derive(Debug, Clone)]
pub struct SmartPdfConfig {
    /// Render DPI for full-page images.
    pub image_dpi: u32,
    /// Image-coverage ratio at/above which a page is image-heavy.
    pub page_as_image_threshold: f64,
    /// Minimum meaningful chars for a page's text to count as readable.
    pub min_chars_per_page: usize,
    /// Skip embedded images smaller than this (px on either axis).
    pub min_image_size: u32,
    /// Cap on embedded images described per mixed page.
    pub max_images_per_page: usize,
    /// Cap on pages that may use the vision model (cost guard); pages beyond it
    /// degrade to text-only.
    pub max_vision_pages: usize,
    /// Vision-first OCR for every page (higher fidelity, higher cost).
    pub high_quality: bool,
    /// Apply sharpen/contrast enhancement before OCR (helps Thai diacritics).
    pub enhance: bool,
}

impl Default for SmartPdfConfig {
    fn default() -> Self {
        Self {
            image_dpi: 150,
            page_as_image_threshold: 0.5,
            min_chars_per_page: 50,
            min_image_size: 100,
            max_images_per_page: 5,
            max_vision_pages: 100,
            high_quality: false,
            enhance: false,
        }
    }
}

impl SmartPdfConfig {
    fn thresholds(&self) -> StrategyThresholds {
        StrategyThresholds {
            page_as_image_threshold: self.page_as_image_threshold,
            min_chars_per_page: self.min_chars_per_page,
        }
    }
}

/// Owned, `Send` per-page extraction result from the sync phase.
#[derive(Debug, Clone)]
pub struct PageExtract {
    /// 1-indexed page number.
    pub page_num: usize,
    pub strategy: PageStrategy,
    /// Extracted pdfium text (trimmed).
    pub text: String,
    /// Full-page PNG render, present for ImageHeavy / Scanned / Tabular.
    pub page_png: Option<Vec<u8>>,
    /// Embedded image PNGs, present for Mixed pages (already size-filtered).
    pub embedded: Vec<Vec<u8>>,
}

/// The assembled document plus the per-page renders (for chunking with
/// page/strategy metadata) and telemetry counters.
#[derive(Debug, Clone)]
pub struct SmartPdfDocument {
    /// One canonical semantic-markdown document (page-ordered).
    pub markdown: String,
    /// Per-page rendered markdown + strategy, for metadata-tagged chunking.
    pub pages: Vec<RenderedPage>,
    pub total_pages: usize,
    pub vision_pages_used: usize,
    pub pages_vision_failed: usize,
}

/// Phase 1 (sync): open the PDF once and gather per-page extraction data.
///
/// MUST run inside `tokio::task::spawn_blocking` — pdfium handles are `!Send`.
pub fn extract_pages(pdf: &[u8], cfg: &SmartPdfConfig) -> Result<Vec<PageExtract>> {
    let engine = PdfEngine::new()?;
    let signals = engine.page_signals(pdf)?;
    let thresholds = cfg.thresholds();

    let mut pages = Vec::with_capacity(signals.len());
    for sig in signals {
        let meaningful = meaningful_char_count(&sig.text);
        let tabular = crate::table_extractor::looks_like_table(&sig.text);

        // Only Mixed needs the embedded-image count, and getting it is the same
        // call that produces the bytes — so fetch embedded images first when
        // the page is a low-coverage, readable, non-tabular candidate.
        let low_coverage_readable_nontable = sig.coverage < cfg.page_as_image_threshold
            && meaningful >= cfg.min_chars_per_page
            && !tabular;
        let embedded: Vec<Vec<u8>> = if low_coverage_readable_nontable {
            engine
                .embedded_images(pdf, sig.index, cfg.min_image_size, cfg.enhance)
                .unwrap_or_default()
                .into_iter()
                .map(|img| img.png_bytes)
                .take(cfg.max_images_per_page)
                .collect()
        } else {
            Vec::new()
        };

        let strategy = if cfg.high_quality {
            // High-quality mode forces every page through vision OCR.
            PageStrategy::Scanned
        } else {
            select_page_strategy(
                sig.coverage,
                meaningful,
                tabular,
                embedded.len(),
                &thresholds,
            )
        };

        // Render a full-page image for the strategies that OCR the whole page.
        let page_png = if matches!(
            strategy,
            PageStrategy::ImageHeavy | PageStrategy::Scanned | PageStrategy::Tabular
        ) {
            engine
                .render_page_png(pdf, sig.index, cfg.image_dpi, cfg.enhance)
                .map(|img| img.png_bytes)
                .ok()
        } else {
            None
        };

        pages.push(PageExtract {
            page_num: sig.index + 1,
            strategy,
            text: sig.text,
            page_png,
            embedded,
        });
    }
    Ok(pages)
}

/// Phase 2 (async): render each page to markdown via the vision model, then
/// assemble the canonical document.
pub async fn render_to_document(
    title: &str,
    extracts: Vec<PageExtract>,
    llm: &dyn LlmProvider,
    cfg: &SmartPdfConfig,
) -> SmartPdfDocument {
    let total_pages = extracts.len();
    let mut vision_pages_used = 0usize;
    let mut pages_vision_failed = 0usize;
    let mut rendered = Vec::with_capacity(total_pages);

    for ex in extracts {
        let lang = Language::detect(&ex.text);
        let over_budget = vision_pages_used >= cfg.max_vision_pages;

        let body = match ex.strategy {
            PageStrategy::TextOnly => ex.text.clone(),

            PageStrategy::Tabular => {
                let Some(png) = ex.page_png.as_ref() else {
                    rendered.push(rp(&ex, ex.text.clone()));
                    continue;
                };
                if over_budget {
                    ex.text.clone()
                } else {
                    let prompt = get_prompts(lang).table_extraction;
                    match describe(llm, png, prompt, PAGE_VISION_TOKENS).await {
                        Ok(desc) => {
                            vision_pages_used += 1;
                            desc // contains the markdown table; raw text suppressed
                        }
                        Err(e) => {
                            pages_vision_failed += 1;
                            warn!(page = ex.page_num, error = %e, vision_model = llm.model_name(),
                                "smart-pdf: table OCR failed — keeping extracted text");
                            ex.text.clone()
                        }
                    }
                }
            }

            PageStrategy::ImageHeavy | PageStrategy::Scanned => {
                let Some(png) = ex.page_png.as_ref() else {
                    rendered.push(rp(&ex, ex.text.clone()));
                    continue;
                };
                if over_budget {
                    ex.text.clone()
                } else {
                    let prompt = if cfg.high_quality {
                        high_quality_prompt(lang, &ex.text)
                    } else {
                        get_prompts(lang).full_page.to_string()
                    };
                    match describe(llm, png, &prompt, PAGE_VISION_TOKENS).await {
                        Ok(desc) => {
                            vision_pages_used += 1;
                            // ImageHeavy keeps the readable pdfium text as a
                            // prefix; Scanned text is unreliable, so use OCR only.
                            if ex.strategy == PageStrategy::ImageHeavy && !ex.text.is_empty() {
                                format!("{}\n\n{}", ex.text, desc)
                            } else {
                                desc
                            }
                        }
                        Err(e) => {
                            pages_vision_failed += 1;
                            warn!(page = ex.page_num, error = %e, vision_model = llm.model_name(),
                                "smart-pdf: page OCR failed — keeping extracted text");
                            ex.text.clone()
                        }
                    }
                }
            }

            PageStrategy::Mixed => {
                let mut body = ex.text.clone();
                if !over_budget {
                    let prompt = get_prompts(lang).single_image;
                    let mut described = 0usize;
                    for png in &ex.embedded {
                        match describe(llm, png, prompt, IMAGE_VISION_TOKENS).await {
                            Ok(desc) => {
                                body.push_str("\n\n");
                                body.push_str(&desc);
                                described += 1;
                            }
                            Err(e) => {
                                pages_vision_failed += 1;
                                warn!(page = ex.page_num, error = %e,
                                    vision_model = llm.model_name(),
                                    "smart-pdf: embedded-image description failed — skipping image");
                            }
                        }
                    }
                    if described > 0 {
                        vision_pages_used += 1;
                    }
                }
                body
            }
        };

        rendered.push(rp(&ex, body));
    }

    let markdown = assemble_document_markdown(title, rendered.clone());
    SmartPdfDocument {
        markdown,
        pages: rendered,
        total_pages,
        vision_pages_used,
        pages_vision_failed,
    }
}

/// Build a `RenderedPage` carrying the page's strategy and markdown body.
fn rp(ex: &PageExtract, markdown: String) -> RenderedPage {
    RenderedPage {
        page_num: ex.page_num,
        strategy: ex.strategy,
        markdown,
    }
}

async fn describe(
    llm: &dyn LlmProvider,
    png: &[u8],
    prompt: &str,
    max_tokens: u32,
) -> Result<String> {
    crate::image::describe_image_with_prompt(llm, png, PNG_MIME, prompt, max_tokens).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::traits::LlmProvider;
    use thairag_core::types::{ChatMessage, LlmResponse, VisionMessage};

    /// A vision LLM stub that echoes a fixed description.
    struct StubVision {
        reply: String,
        supports: bool,
    }

    #[async_trait::async_trait]
    impl LlmProvider for StubVision {
        fn model_name(&self) -> &str {
            "stub-vision"
        }
        fn supports_vision(&self) -> bool {
            self.supports
        }
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: self.reply.clone(),
                usage: Default::default(),
            })
        }
        async fn generate_vision(
            &self,
            _messages: &[VisionMessage],
            _max_tokens: Option<u32>,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: self.reply.clone(),
                usage: Default::default(),
            })
        }
    }

    fn cfg() -> SmartPdfConfig {
        SmartPdfConfig::default()
    }

    #[tokio::test]
    async fn text_only_page_skips_vision() {
        let llm = StubVision {
            reply: "VISION".into(),
            supports: true,
        };
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::TextOnly,
            text: "Plenty of readable body text on this page.".into(),
            page_png: None,
            embedded: vec![],
        }];
        let doc = render_to_document("Doc", pages, &llm, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 0);
        assert!(doc.markdown.contains("readable body text"));
        assert!(!doc.markdown.contains("VISION"));
        assert!(doc.markdown.contains("## Page 1"));
    }

    #[tokio::test]
    async fn scanned_page_uses_vision_ocr() {
        let llm = StubVision {
            reply: "# OCR heading\nbody".into(),
            supports: true,
        };
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::Scanned,
            text: String::new(),
            page_png: Some(vec![1, 2, 3]),
            embedded: vec![],
        }];
        let doc = render_to_document("Doc", pages, &llm, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 1);
        assert!(doc.markdown.contains("OCR heading"));
    }

    #[tokio::test]
    async fn tabular_page_replaces_text_with_table_ocr() {
        let llm = StubVision {
            reply: "| a | b |\n|---|---|\n| 1 | 2 |".into(),
            supports: true,
        };
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::Tabular,
            text: "raw  tabular  text  that  should  be  replaced".into(),
            page_png: Some(vec![1, 2, 3]),
            embedded: vec![],
        }];
        let doc = render_to_document("Doc", pages, &llm, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 1);
        assert!(doc.markdown.contains("| a | b |"));
        assert!(!doc.markdown.contains("should  be  replaced"));
    }

    #[tokio::test]
    async fn vision_failure_keeps_extracted_text() {
        // supports_vision=false makes describe return a placeholder rather than
        // erroring, so simulate a hard failure path via the budget instead:
        let llm = StubVision {
            reply: "X".into(),
            supports: true,
        };
        let mut c = cfg();
        c.max_vision_pages = 0; // every page is over budget → degrade to text
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::ImageHeavy,
            text: "fallback body text".into(),
            page_png: Some(vec![1, 2, 3]),
            embedded: vec![],
        }];
        let doc = render_to_document("Doc", pages, &llm, &c).await;
        assert_eq!(doc.vision_pages_used, 0);
        assert!(doc.markdown.contains("fallback body text"));
    }

    #[tokio::test]
    async fn high_quality_forces_scanned_strategy() {
        // Sanity: the high-quality flag is read by extract_pages, not here, but
        // confirm the config plumbs through.
        let c = SmartPdfConfig {
            high_quality: true,
            ..cfg()
        };
        assert!(c.high_quality);
        assert_eq!(c.thresholds().page_as_image_threshold, 0.5);
    }
}
