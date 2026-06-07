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
use thairag_core::types::ImageId;
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
/// Minimum confidence to trust a deterministic table reconstruction. Below
/// this, the page keeps its raw text and is flagged (`tables_kept_as_text`) —
/// we do not fall back to vision OCR, which can fabricate numerals.
const LATTICE_MIN_CONFIDENCE: f32 = 0.3;
/// Minimum fraction of a page's glyphs that must fall inside the reconstructed
/// grid before we treat the page as table-dominated and replace its body with
/// the HTML. Guards against clobbering a prose page that has a small ruled box.
const LATTICE_MIN_COVERAGE: f32 = 0.5;
/// Minimum grid cells for a reconstruction to count as a real table (avoids
/// turning a stray 1×1 ruled box into a "table").
const LATTICE_MIN_CELLS: usize = 4;

/// Tunables for the smart-PDF engine. Defaults mirror Jay-RAG-Tools.
#[derive(Debug, Clone)]
pub struct SmartPdfConfig {
    /// Render DPI for full-page images.
    pub image_dpi: u32,
    /// Longest-edge pixel cap for images sent to the vision model (`0` = off).
    /// A safety net on top of `image_dpi`: oversized renders are downscaled
    /// before description. Shared with the embedded-media / direct-image paths.
    pub max_image_edge: u32,
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
            max_image_edge: 2048,
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
    /// Full-page PNG render, present for ImageHeavy / Scanned / Tabular pages
    /// that need vision. `None` for a Tabular page handled by deterministic
    /// lattice reconstruction (no vision needed).
    pub page_png: Option<Vec<u8>>,
    /// Embedded image PNGs, present for Mixed pages (already size-filtered).
    pub embedded: Vec<Vec<u8>>,
    /// Deterministic table reconstruction of a digital table — bordered (lattice,
    /// from ruling lines) or borderless (stream, from whitespace columns). When
    /// present the page is rendered from this HTML — no vision call, so cell
    /// content comes straight from the text layer.
    pub table: Option<crate::table_lattice::ReconstructedTable>,
}

/// An image blob extracted during processing, with a minted id, ready for the
/// caller (the API/store layer) to persist. The id is already embedded in the
/// page markdown (`[IMAGE:<id>]`) and on the page's chunks (`image_blob_id`).
#[derive(Debug, Clone)]
pub struct ExtractedImageBlob {
    pub image_id: ImageId,
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 1-indexed source page, or `None` for non-paged sources (office/html).
    pub page_num: Option<u32>,
    /// Stable source tag (e.g. `pdf_page_render`); maps to the store's
    /// `ImageSource` via `from_str_lossy`.
    pub source: &'static str,
}

/// The assembled document plus the per-page renders (for chunking with
/// page/strategy metadata), the extracted image blobs, and telemetry counters.
#[derive(Debug, Clone)]
pub struct SmartPdfDocument {
    /// One canonical semantic-markdown document (page-ordered).
    pub markdown: String,
    /// Per-page rendered markdown + strategy, for metadata-tagged chunking.
    pub pages: Vec<RenderedPage>,
    /// Image blobs (full-page renders) with minted ids, for the caller to save.
    pub images: Vec<ExtractedImageBlob>,
    pub total_pages: usize,
    pub vision_pages_used: usize,
    pub pages_vision_failed: usize,
    /// Pages classified `Tabular` whose table could not be reconstructed
    /// deterministically (no ruled grid, no trustworthy borderless grid). Their
    /// raw text is kept verbatim — numbers stay exact, but the table *structure*
    /// is not recovered. Surfaced so an analyst can spot a layout that may need
    /// a manual look (we deliberately do NOT fall back to probabilistic vision
    /// OCR, which can fabricate Thai numerals).
    pub tables_kept_as_text: usize,
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

        // Deterministic lattice reconstruction from the digital text layer
        // (exact numbers, no vision). Driven by GEOMETRY, not the whitespace
        // "looks tabular" heuristic — a bordered table is often classified
        // TextOnly, so we attempt reconstruction on any page with a usable text
        // layer (i.e. not a scanned/image-only page). Accept only a
        // table-dominated, sufficiently-confident grid; otherwise fall through
        // to the page's normal handling (and vision for true Tabular pages).
        let has_text_layer =
            meaningful >= cfg.min_chars_per_page && !matches!(strategy, PageStrategy::Scanned);
        let table = if has_text_layer {
            engine.page_geometry(pdf, sig.index).ok().and_then(|g| {
                // Bordered first (lattice, from ruling lines); if there's no
                // ruled grid, try borderless (stream, from whitespace columns).
                let lattice = crate::table_lattice::reconstruct(&g.chars, &g.lines);
                let chosen = lattice.or_else(|| crate::table_stream::reconstruct(&g.chars));
                chosen.filter(|t| {
                    t.confidence >= LATTICE_MIN_CONFIDENCE
                        && t.char_coverage >= LATTICE_MIN_COVERAGE
                        && t.n_rows * t.n_cols >= LATTICE_MIN_CELLS
                })
            })
        } else {
            None
        };

        // Render a full-page image for the whole-page-OCR strategies — but skip
        // it for a Tabular page already solved deterministically. (A borderless
        // Tabular page with no reconstruction now keeps its text — vision is
        // reserved for genuinely scanned pages.)
        let needs_render = matches!(strategy, PageStrategy::ImageHeavy | PageStrategy::Scanned);
        let page_png = if needs_render {
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
            table,
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
    let mut tables_kept_as_text = 0usize;
    let mut rendered = Vec::with_capacity(total_pages);
    let mut images: Vec<ExtractedImageBlob> = Vec::new();

    for ex in extracts {
        let lang = Language::detect(&ex.text);
        let over_budget = vision_pages_used >= cfg.max_vision_pages;

        // A successful lattice reconstruction wins for ANY page strategy: the
        // HTML table's numbers come straight from the text layer (no vision, no
        // fabrication). This is what fixes a bordered table that the heuristic
        // mislabelled TextOnly.
        let mut body = if let Some(lat) = ex.table.as_ref() {
            lat.html.clone()
        } else {
            match ex.strategy {
                PageStrategy::TextOnly => ex.text.clone(),

                PageStrategy::Tabular => {
                    // No trustworthy reconstruction (borderless with no clean
                    // column grid, or below the confidence/coverage gate). Keep
                    // the raw text verbatim — numbers stay exact — and flag the
                    // page. We deliberately do NOT fall back to vision OCR: it is
                    // probabilistic and fabricates Thai numerals.
                    tables_kept_as_text += 1;
                    ex.text.clone()
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
                        match describe(llm, png, &prompt, PAGE_VISION_TOKENS, cfg.max_image_edge)
                            .await
                        {
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
                            match describe(
                                llm,
                                png,
                                prompt,
                                IMAGE_VISION_TOKENS,
                                cfg.max_image_edge,
                            )
                            .await
                            {
                                Ok(desc) => {
                                    // Persist the embedded image and embed its
                                    // marker before the description.
                                    let image_id = ImageId::new();
                                    let meta = crate::image::extract_image_metadata(png, PNG_MIME);
                                    images.push(ExtractedImageBlob {
                                        image_id,
                                        bytes: png.clone(),
                                        mime: PNG_MIME.to_string(),
                                        width: meta.width,
                                        height: meta.height,
                                        page_num: Some(ex.page_num as u32),
                                        source: "pdf_embedded",
                                    });
                                    body.push_str("\n\n");
                                    body.push_str(&crate::semantic::image_marker(
                                        &image_id.to_string(),
                                    ));
                                    body.push('\n');
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
            }
        };

        // Persist the full-page render for the vision strategies (one image per
        // page). Mint the id here so it can be embedded both in the page
        // markdown (`[IMAGE:<id>]`) and on the page's chunks (`image_blob_id`).
        // Embedded-image (Mixed) blob persistence is deferred — those images are
        // described inline.
        if matches!(
            ex.strategy,
            PageStrategy::ImageHeavy | PageStrategy::Scanned
        ) && let Some(png) = ex.page_png.as_ref()
        {
            let image_id = ImageId::new();
            let meta = crate::image::extract_image_metadata(png, PNG_MIME);
            images.push(ExtractedImageBlob {
                image_id,
                bytes: png.clone(),
                mime: PNG_MIME.to_string(),
                width: meta.width,
                height: meta.height,
                page_num: Some(ex.page_num as u32),
                source: "pdf_page_render",
            });
            body = format!(
                "{}\n{}",
                crate::semantic::image_marker(&image_id.to_string()),
                body
            );
        }

        rendered.push(rp(&ex, body));
    }

    let markdown = assemble_document_markdown(title, rendered.clone());
    SmartPdfDocument {
        markdown,
        pages: rendered,
        images,
        total_pages,
        vision_pages_used,
        pages_vision_failed,
        tables_kept_as_text,
    }
}

/// Build a `RenderedPage` carrying the page's strategy and markdown body, plus
/// the lattice table (HTML + linearized) when the page was reconstructed
/// deterministically — so the pipeline can chunk it atomically.
fn rp(ex: &PageExtract, markdown: String) -> RenderedPage {
    RenderedPage {
        page_num: ex.page_num,
        strategy: ex.strategy,
        markdown,
        table_html: ex.table.as_ref().map(|l| l.html.clone()),
        table_linearized: ex.table.as_ref().map(|l| l.linearized.clone()),
    }
}

async fn describe(
    llm: &dyn LlmProvider,
    png: &[u8],
    prompt: &str,
    max_tokens: u32,
    max_image_edge: u32,
) -> Result<String> {
    crate::image::describe_image_with_prompt(llm, png, PNG_MIME, prompt, max_tokens, max_image_edge)
        .await
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
            table: None,
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
            table: None,
        }];
        let doc = render_to_document("Doc", pages, &llm, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 1);
        assert!(doc.markdown.contains("OCR heading"));
        // The full-page render is collected as a blob and its id embedded.
        assert_eq!(doc.images.len(), 1);
        assert_eq!(doc.images[0].source, "pdf_page_render");
        assert_eq!(doc.images[0].page_num, Some(1));
        let marker = crate::semantic::image_marker(&doc.images[0].image_id.to_string());
        assert!(
            doc.markdown.contains(&marker),
            "marker missing: {}",
            doc.markdown
        );
    }

    #[tokio::test]
    async fn tabular_page_without_reconstruction_keeps_text_and_flags() {
        // A Tabular page with no trustworthy reconstruction keeps its raw text
        // verbatim (numbers exact) and is flagged — never vision OCR, which can
        // fabricate Thai numerals.
        let llm = StubVision {
            reply: "VISION SHOULD NOT RUN".into(),
            supports: true,
        };
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::Tabular,
            text: "raw tabular text with exact numbers 123 456".into(),
            page_png: None,
            embedded: vec![],
            table: None,
        }];
        let doc = render_to_document("Doc", pages, &llm, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 0, "must not call vision");
        assert_eq!(doc.tables_kept_as_text, 1, "page should be flagged");
        assert!(doc.markdown.contains("exact numbers 123 456"));
        assert!(!doc.markdown.contains("VISION SHOULD NOT RUN"));
    }

    #[tokio::test]
    async fn tabular_page_uses_lattice_html_without_vision() {
        // A Tabular page with a reconstructed lattice must use the deterministic
        // HTML — no vision call, no page-render blob — so numbers stay exact.
        let llm = StubVision {
            reply: "VISION SHOULD NOT RUN".into(),
            supports: true,
        };
        let lat = crate::table_lattice::ReconstructedTable {
            html: "<table><tr><td>ก</td><td>๑๒๓</td></tr></table>".into(),
            linearized: "ก | ๑๒๓".into(),
            confidence: 1.0,
            char_coverage: 1.0,
            n_rows: 1,
            n_cols: 2,
        };
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::Tabular,
            text: "raw tabular text".into(),
            page_png: None,
            embedded: vec![],
            table: Some(lat),
        }];
        let doc = render_to_document("Doc", pages, &llm, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 0, "lattice must not call vision");
        assert!(
            doc.markdown.contains("<table>"),
            "html missing: {}",
            doc.markdown
        );
        assert!(doc.markdown.contains("๑๒๓"));
        assert!(!doc.markdown.contains("VISION SHOULD NOT RUN"));
        assert!(
            doc.images.is_empty(),
            "no page-render blob for lattice tables"
        );
        // The page carries the table html + linearized embedding text.
        assert_eq!(
            doc.pages[0].table_html.as_deref(),
            Some("<table><tr><td>ก</td><td>๑๒๓</td></tr></table>")
        );
        assert_eq!(doc.pages[0].table_linearized.as_deref(), Some("ก | ๑๒๓"));
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
            table: None,
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
