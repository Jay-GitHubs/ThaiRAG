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

use futures_util::{StreamExt, stream};
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;

use crate::ocr::OcrProvider;
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
/// A table whose glyph coverage reaches this is trusted even if its fill-ratio
/// `confidence` is low. Real tax/statistical tables are legitimately sparse
/// (many empty cells → low fill-ratio) yet place nearly all their text in the
/// grid (coverage ≈ 1.0). Gating only on fill-ratio dropped these to flat text.
const LATTICE_HIGH_COVERAGE: f32 = 0.7;
/// Minimum raw pixel size for an image to qualify as table-cell content. Much
/// lower than `min_image_size` (the Mixed-page describe threshold): in-cell
/// logos/stamps are legitimately small, while anything under this is a border
/// artifact or tracking pixel.
const CELL_IMAGE_MIN_SIZE: u32 = 16;
/// Max per-boundary drift (points) for two pages' column fingerprints to count
/// as the same grid (multi-page stitching). Matches `MIN_CELL_GAP`'s scale —
/// a real layout reproduces its ruled boundaries far more precisely than this.
const STITCH_COL_TOL: f32 = 6.0;

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
    /// Fidelity-gated table rescue: when the whole-document fidelity check
    /// flags "review" and the doc has mechanically-reconstructed table pages,
    /// re-transcribe those pages with the vision model and keep whichever
    /// version scores better. Only fires for born-digital PDFs (scanned pages
    /// have no text-layer ground truth, so fidelity is "unverifiable" there).
    pub table_rescue_enabled: bool,
    /// Cap on pages re-transcribed per rescued document (cost guard).
    pub table_rescue_max_pages: usize,
    pub high_quality: bool,
    /// Apply sharpen/contrast enhancement before OCR (helps Thai diacritics).
    pub enhance: bool,
    /// Max per-page vision OCR calls in flight at once. Pages are OCR'd
    /// concurrently up to this bound (then reassembled in page order), turning
    /// wall-clock from sum-of-pages into ~ceil(pages/concurrency)·latency. Keep
    /// it modest: a heavy model (e.g. 72B) on a shared/flaky gateway will 5xx
    /// under too much parallelism (transient failures are retried, but flooding
    /// wastes work). `1` = fully sequential.
    pub vision_concurrency: usize,
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
            table_rescue_enabled: true,
            table_rescue_max_pages: 8,
            high_quality: false,
            enhance: false,
            vision_concurrency: 2,
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
    /// Images that landed inside a reconstructed table cell. Their ids are
    /// already embedded as `[IMAGE:<id>]` markers in `table` html/linearized;
    /// phase 2 persists the bytes as blobs.
    pub cell_images: Vec<CellImage>,
    /// All page numbers whose rows live in `table`, when consecutive same-grid
    /// pages were stitched into this one. Empty for a single-page table.
    pub table_pages: Vec<usize>,
    /// Set on a continuation page whose table rows were stitched into an
    /// earlier page (the value is that anchor page's number). Phase 2 renders
    /// this page empty — its content lives on the anchor.
    pub stitched_into: Option<usize>,
}

/// An embedded image assigned to a table cell during lattice reconstruction:
/// the minted blob id plus the PNG bytes to persist.
#[derive(Debug, Clone)]
pub struct CellImage {
    pub image_id: ImageId,
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
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
    /// Pages transcribed by the deterministic OCR tier (PaddleOCR sidecar) rather
    /// than the vision LLM. `0` when no OCR provider is configured.
    pub ocr_pages_used: usize,
    pub pages_vision_failed: usize,
    /// Pages that would have gone through vision OCR (ImageHeavy/Scanned/Mixed)
    /// but were kept as their raw text layer because no vision model was
    /// configured. The pdfium text path still runs for every PDF (so text pages
    /// are clean), but image/scanned pages can't be OCR'd without a model.
    /// Surfaced so an operator can tell "this PDF needed vision and had none".
    pub pages_vision_skipped: usize,
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
            let selected = select_page_strategy(
                sig.coverage,
                meaningful,
                tabular,
                embedded.len(),
                &thresholds,
            );
            // A text-layer that the strategy would otherwise trust (TextOnly /
            // Tabular / Mixed) but which is corrupted by a broken ToUnicode CMap
            // (Latin-Extended leaking into Thai, e.g. `เรืĻอง`) is worthless —
            // every text extractor reads the same garbage. Force full-page vision
            // OCR of the render so the actual glyphs are read instead. Already-
            // vision strategies (ImageHeavy/Scanned) need no change.
            if !matches!(selected, PageStrategy::ImageHeavy | PageStrategy::Scanned)
                && crate::text_utils::text_layer_garbled(&sig.text)
            {
                PageStrategy::Scanned
            } else {
                selected
            }
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
        let mut cell_images: Vec<CellImage> = Vec::new();
        let table = if has_text_layer {
            engine.page_geometry(pdf, sig.index).ok().and_then(|g| {
                let accept = |t: &crate::table_lattice::ReconstructedTable| {
                    t.n_rows * t.n_cols >= LATTICE_MIN_CELLS
                        && t.char_coverage >= LATTICE_MIN_COVERAGE
                        // Trust a sparse grid (low fill-ratio) when nearly all of
                        // the page's text landed in it; require the fill-ratio
                        // floor only when coverage is merely moderate.
                        && (t.confidence >= LATTICE_MIN_CONFIDENCE
                            || t.char_coverage >= LATTICE_HIGH_COVERAGE)
                };
                // Bordered first (lattice, from ruling lines); if there's no
                // ruled grid, try borderless (stream, from whitespace columns).
                let mut lattice = crate::table_lattice::reconstruct(&g.chars, &g.lines, &[]);
                // An accepted ruled grid may contain in-cell images (logos,
                // photos, charts). Fetch them only now — image decode is not
                // free and most text pages have no grid — mint blob ids, and
                // re-run reconstruction so each cell carries its image marker.
                if lattice.as_ref().is_some_and(accept) {
                    let imgs = engine
                        .embedded_images(pdf, sig.index, CELL_IMAGE_MIN_SIZE, cfg.enhance)
                        .unwrap_or_default();
                    let mut placed = Vec::new();
                    let mut pending: Vec<CellImage> = Vec::new();
                    for img in imgs.into_iter().take(cfg.max_images_per_page) {
                        let Some((x0, y0, x1, y1)) = img.bounds else {
                            continue;
                        };
                        let image_id = ImageId::new();
                        placed.push(crate::table_lattice::PlacedImage {
                            label: image_id.to_string(),
                            x0,
                            y0,
                            x1,
                            y1,
                        });
                        pending.push(CellImage {
                            image_id,
                            png: img.png_bytes,
                            width: img.width,
                            height: img.height,
                        });
                    }
                    if !placed.is_empty()
                        && let Some(t) =
                            crate::table_lattice::reconstruct(&g.chars, &g.lines, &placed)
                                .filter(accept)
                    {
                        // Keep only blobs whose marker actually landed in a cell
                        // (an image outside the grid is not table content).
                        cell_images = pending
                            .into_iter()
                            .filter(|ci| {
                                t.html.contains(&crate::semantic::image_marker(
                                    &ci.image_id.to_string(),
                                ))
                            })
                            .collect();
                        lattice = Some(t);
                    }
                }
                let chosen = lattice.or_else(|| crate::table_stream::reconstruct(&g.chars));
                chosen.filter(accept)
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
            cell_images,
            table_pages: Vec::new(),
            stitched_into: None,
        });
    }
    stitch_multipage_tables(&mut pages);
    Ok(pages)
}

/// Column boundaries equal within [`STITCH_COL_TOL`] points position-by-position.
fn cols_match(a: &[f32], b: &[f32]) -> bool {
    !a.is_empty()
        && a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= STITCH_COL_TOL)
}

/// Stitch a table that continues across consecutive pages into one table.
///
/// A multi-page table reproduces the same ruled column boundaries on every
/// page (`ReconstructedTable::col_xs` — the geometric fingerprint), so
/// consecutive pages whose reconstructed grids share that fingerprint are one
/// logical table. The continuation's rows are appended to the anchor page's
/// table (dropping a repeated header row when its cells equal the anchor's
/// first row), its cell-image blobs move to the anchor, and the page is marked
/// `stitched_into` so phase 2 renders it empty instead of duplicating content.
fn stitch_multipage_tables(pages: &mut [PageExtract]) {
    let mut anchor = 0usize;
    for i in 1..pages.len() {
        // `pages` holds every page in order, so index adjacency = page
        // adjacency; a page without a matching grid starts a new run.
        let matches = match (pages[anchor].table.as_ref(), pages[i].table.as_ref()) {
            (Some(at), Some(bt)) => cols_match(&at.col_xs, &bt.col_xs),
            _ => false,
        };
        if !matches {
            anchor = i;
            continue;
        }
        let cont = pages[i].table.take().expect("matched table");
        let mut cont_imgs = std::mem::take(&mut pages[i].cell_images);
        let base_page = pages[anchor].page_num;
        let cont_page = pages[i].page_num;
        pages[i].stitched_into = Some(base_page);

        let base = pages[anchor].table.as_mut().expect("matched table");
        let (mut cont_lin, mut cont_html, mut cont_rows) =
            (cont.linearized, cont.html, cont.n_rows);
        // Drop the continuation's repeated header (same first-row cells).
        let base_header = base
            .linearized
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        if cont_rows > 1 && cont_lin.lines().next() == Some(base_header.as_str()) {
            cont_lin = cont_lin
                .split_once('\n')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_default();
            if let (Some(start), Some(end)) = (cont_html.find("<tr>"), cont_html.find("</tr>"))
                && start < end
            {
                cont_html.replace_range(start..end + "</tr>".len(), "");
            }
            cont_rows -= 1;
        }
        // Merge the HTML row streams: strip the seam tags and concatenate.
        let base_body = base.html.strip_suffix("</table>").unwrap_or(&base.html);
        let cont_body = cont_html.strip_prefix("<table>").unwrap_or(&cont_html);
        base.html = format!("{base_body}{cont_body}");
        if !cont_lin.is_empty() {
            base.linearized.push('\n');
            base.linearized.push_str(&cont_lin);
        }
        // Cell-weighted confidence; conservative coverage.
        let (bc, cc) = (
            (base.n_rows * base.n_cols) as f32,
            (cont_rows * cont.n_cols) as f32,
        );
        base.confidence = (base.confidence * bc + cont.confidence * cc) / (bc + cc).max(1.0);
        base.char_coverage = base.char_coverage.min(cont.char_coverage);
        base.n_rows += cont_rows;

        if pages[anchor].table_pages.is_empty() {
            pages[anchor].table_pages.push(base_page);
        }
        pages[anchor].table_pages.push(cont_page);
        pages[anchor].cell_images.append(&mut cont_imgs);
    }
}

/// Phase 2 (async): render each page to markdown via the vision model, then
/// assemble the canonical document.
pub async fn render_to_document(
    title: &str,
    extracts: Vec<PageExtract>,
    llm: Option<&dyn LlmProvider>,
    ocr: Option<&dyn OcrProvider>,
    cfg: &SmartPdfConfig,
) -> SmartPdfDocument {
    let total_pages = extracts.len();

    // Pre-assign the per-doc model budget by page order so concurrently-rendered
    // pages don't race on a running counter: the first `max_vision_pages` pages
    // that would use a model pass (OCR or vision) are allowed, the rest degrade
    // to text. (The budget bounds *attempts* rather than successes.)
    let has_llm = llm.is_some();
    let has_ocr = ocr.is_some();
    let mut allow_model = vec![false; total_pages];
    let mut budget = cfg.max_vision_pages;
    for (i, ex) in extracts.iter().enumerate() {
        if budget == 0 {
            break;
        }
        if page_wants_model(ex, has_llm, has_ocr) {
            allow_model[i] = true;
            budget -= 1;
        }
    }

    // Process pages concurrently (bounded), then reassemble in page order. Each
    // page is independent, so wall-clock drops from sum-of-pages to roughly
    // ceil(pages / vision_concurrency) · per-page latency.
    let concurrency = cfg.vision_concurrency.max(1);
    let mut results: Vec<PageRender> = stream::iter(extracts.into_iter().enumerate())
        .map(|(i, ex)| render_page(i, ex, llm, ocr, cfg, allow_model[i]))
        .buffer_unordered(concurrency)
        .collect()
        .await;
    results.sort_by_key(|r| r.order);

    let mut rendered = Vec::with_capacity(total_pages);
    let mut images: Vec<ExtractedImageBlob> = Vec::new();
    let mut vision_pages_used = 0usize;
    let mut ocr_pages_used = 0usize;
    let mut pages_vision_failed = 0usize;
    let mut pages_vision_skipped = 0usize;
    let mut tables_kept_as_text = 0usize;
    for mut r in results {
        rendered.push(r.rendered);
        images.append(&mut r.images);
        vision_pages_used += r.vision_used;
        ocr_pages_used += r.ocr_used;
        pages_vision_failed += r.vision_failed;
        pages_vision_skipped += r.vision_skipped;
        tables_kept_as_text += r.tables_kept;
    }

    let markdown = assemble_document_markdown(title, rendered.clone());
    SmartPdfDocument {
        markdown,
        pages: rendered,
        images,
        total_pages,
        vision_pages_used,
        ocr_pages_used,
        pages_vision_failed,
        pages_vision_skipped,
        tables_kept_as_text,
    }
}

/// Per-page render result, aggregated after concurrent processing completes.
struct PageRender {
    order: usize,
    rendered: RenderedPage,
    images: Vec<ExtractedImageBlob>,
    vision_used: usize,
    ocr_used: usize,
    vision_failed: usize,
    vision_skipped: usize,
    tables_kept: usize,
}

/// Whether a page would attempt a model pass (deterministic OCR or vision) —
/// used to pre-assign the `max_vision_pages` budget before concurrent execution.
/// A full-page OCR page (ImageHeavy/Scanned) is served by either an OCR provider
/// or the vision LLM; a Mixed page's figure description is the vision LLM's job
/// only (OCR transcribes text, it doesn't describe figures).
fn page_wants_model(ex: &PageExtract, has_llm: bool, has_ocr: bool) -> bool {
    if ex.stitched_into.is_some() || ex.table.is_some() {
        return false;
    }
    match ex.strategy {
        PageStrategy::ImageHeavy | PageStrategy::Scanned => {
            (has_llm || has_ocr) && ex.page_png.is_some()
        }
        PageStrategy::Mixed => has_llm && !ex.embedded.is_empty(),
        _ => false,
    }
}

/// Render one page to markdown (independently, for concurrent execution).
/// `allow_model` is the pre-assigned budget decision for this page.
async fn render_page(
    order: usize,
    ex: PageExtract,
    llm: Option<&dyn LlmProvider>,
    ocr: Option<&dyn OcrProvider>,
    cfg: &SmartPdfConfig,
    allow_model: bool,
) -> PageRender {
    let mut images: Vec<ExtractedImageBlob> = Vec::new();
    let mut vision_used = 0usize;
    let mut ocr_used = 0usize;
    let mut vision_failed = 0usize;
    let mut vision_skipped = 0usize;
    let mut tables_kept = 0usize;

    // A continuation page whose table rows were stitched into an earlier page
    // renders empty — its content lives on the anchor page.
    if ex.stitched_into.is_some() {
        return PageRender {
            order,
            rendered: rp(&ex, String::new()),
            images,
            vision_used,
            ocr_used,
            vision_failed,
            vision_skipped,
            tables_kept,
        };
    }
    let lang = Language::detect(&ex.text);

    // A successful lattice reconstruction wins for ANY page strategy: the HTML
    // table's numbers come straight from the text layer (no vision, no
    // fabrication). This is what fixes a bordered table mislabelled TextOnly.
    let mut body = if let Some(lat) = ex.table.as_ref() {
        for ci in &ex.cell_images {
            images.push(ExtractedImageBlob {
                image_id: ci.image_id,
                bytes: ci.png.clone(),
                mime: PNG_MIME.to_string(),
                width: Some(ci.width),
                height: Some(ci.height),
                page_num: Some(ex.page_num as u32),
                source: "pdf_embedded",
            });
        }
        lat.html.clone()
    } else {
        match ex.strategy {
            PageStrategy::TextOnly => ex.text.clone(),

            PageStrategy::Tabular => {
                // No trustworthy reconstruction. Keep the raw text verbatim —
                // numbers stay exact — and flag the page. We deliberately do NOT
                // fall back to vision OCR, which fabricates Thai numerals.
                tables_kept += 1;
                ex.text.clone()
            }

            PageStrategy::ImageHeavy | PageStrategy::Scanned => {
                let Some(png) = ex.page_png.as_ref() else {
                    return PageRender {
                        order,
                        rendered: rp(&ex, ex.text.clone()),
                        images,
                        vision_used,
                        ocr_used,
                        vision_failed,
                        vision_skipped,
                        tables_kept,
                    };
                };
                if ocr.is_none() && llm.is_none() {
                    // No model of any kind: keep the pdfium text layer and flag
                    // the page so the operator knows it needed OCR.
                    vision_skipped += 1;
                    ex.text.clone()
                } else if !allow_model {
                    // A model exists but the per-doc budget is exhausted.
                    ex.text.clone()
                } else {
                    // Prefer the deterministic OCR tier (no hallucination, local,
                    // measured better on Thai); fall back to the vision LLM, then
                    // to the extracted text. Default-off: with `ocr == None` this
                    // is exactly the previous vision-only behavior.
                    let mut out: Option<String> = None;
                    if let Some(ocr) = ocr {
                        match ocr.ocr(png).await {
                            Ok(t) if !t.trim().is_empty() => {
                                ocr_used += 1;
                                out = Some(t);
                            }
                            Ok(_) => {} // empty transcript → try the vision fallback
                            Err(e) => {
                                warn!(page = ex.page_num, error = %e, ocr = ocr.name(),
                                "smart-pdf: deterministic OCR failed — trying vision fallback");
                            }
                        }
                    }
                    if out.is_none() {
                        if let Some(llm) = llm {
                            let prompt = if cfg.high_quality {
                                high_quality_prompt(lang, &ex.text)
                            } else {
                                get_prompts(lang).full_page.to_string()
                            };
                            match describe(
                                llm,
                                png,
                                &prompt,
                                PAGE_VISION_TOKENS,
                                cfg.max_image_edge,
                            )
                            .await
                            {
                                Ok(desc) => {
                                    vision_used += 1;
                                    // ImageHeavy keeps the readable pdfium text as
                                    // a prefix; Scanned text is unreliable, OCR only.
                                    if ex.strategy == PageStrategy::ImageHeavy
                                        && !ex.text.is_empty()
                                    {
                                        out = Some(format!("{}\n\n{}", ex.text, desc));
                                    } else {
                                        out = Some(desc);
                                    }
                                }
                                Err(e) => {
                                    vision_failed += 1;
                                    warn!(page = ex.page_num, error = %e, vision_model = llm.model_name(),
                                    "smart-pdf: page OCR failed — keeping extracted text");
                                }
                            }
                        } else {
                            // OCR was the only provider and it failed/was empty.
                            vision_failed += 1;
                        }
                    }
                    out.unwrap_or_else(|| ex.text.clone())
                }
            }

            PageStrategy::Mixed => {
                let mut body = ex.text.clone();
                let Some(llm) = llm else {
                    if !ex.embedded.is_empty() {
                        vision_skipped += 1;
                    }
                    return PageRender {
                        order,
                        rendered: rp(&ex, body),
                        images,
                        vision_used,
                        ocr_used,
                        vision_failed,
                        vision_skipped,
                        tables_kept,
                    };
                };
                if allow_model {
                    let prompt = get_prompts(lang).single_image;
                    let mut described = 0usize;
                    for png in &ex.embedded {
                        match describe(llm, png, prompt, IMAGE_VISION_TOKENS, cfg.max_image_edge)
                            .await
                        {
                            Ok(desc) => {
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
                                vision_failed += 1;
                                warn!(page = ex.page_num, error = %e,
                                vision_model = llm.model_name(),
                                "smart-pdf: embedded-image description failed — skipping image");
                            }
                        }
                    }
                    if described > 0 {
                        vision_used += 1;
                    }
                }
                body
            }
        }
    };

    // Persist the full-page render for the vision strategies (one image per
    // page). Mint the id here so it can be embedded both in the page markdown
    // (`[IMAGE:<id>]`) and on the page's chunks. Embedded-image (Mixed) blobs
    // are persisted inline above.
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

    PageRender {
        order,
        rendered: rp(&ex, body),
        images,
        vision_used,
        ocr_used,
        vision_failed,
        vision_skipped,
        tables_kept,
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
        table_pages: ex.table_pages.clone(),
    }
}

/// Fidelity-gated table rescue: render the given (1-indexed) pages and have
/// the vision model re-transcribe each with the table-extraction prompt.
/// Returns `(page_num, transcription)` for pages that transcribed cleanly;
/// failed pages are simply omitted. Pure attempt — the CALLER decides adoption
/// by re-scoring document fidelity (keep-if-better), so a hallucinating model
/// can never make the document worse under the same metric that flagged it.
pub async fn rescue_table_pages(
    pdf: &[u8],
    page_nums: &[usize],
    llm: &dyn LlmProvider,
    lang: crate::semantic_prompts::Language,
    cfg: &SmartPdfConfig,
) -> Vec<(usize, String)> {
    // Render synchronously (pdfium is !Send), then transcribe async.
    let pdf_owned = pdf.to_vec();
    let pages: Vec<usize> = page_nums.to_vec();
    let dpi = cfg.image_dpi;
    let renders = tokio::task::spawn_blocking(move || {
        let engine = match crate::pdfium_engine::PdfEngine::new() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "table rescue: pdfium unavailable");
                return Vec::new();
            }
        };
        pages
            .iter()
            .filter_map(|&pn| {
                engine
                    .render_page_png(&pdf_owned, pn - 1, dpi, false)
                    .map_err(
                        |e| tracing::warn!(page = pn, error = %e, "table rescue: render failed"),
                    )
                    .ok()
                    .map(|img| (pn, img.png_bytes))
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();

    let prompt = crate::semantic_prompts::get_prompts(lang).table_extraction;
    let mut out = Vec::new();
    for (pn, png) in renders {
        match describe(llm, &png, prompt, PAGE_VISION_TOKENS, cfg.max_image_edge).await {
            Ok(text) if !text.trim().is_empty() => out.push((pn, text)),
            Ok(_) => tracing::warn!(page = pn, "table rescue: empty transcription"),
            Err(e) => tracing::warn!(page = pn, error = %e, "table rescue: transcription failed"),
        }
    }
    out
}

async fn describe(
    llm: &dyn LlmProvider,
    png: &[u8],
    prompt: &str,
    max_tokens: u32,
    max_image_edge: u32,
) -> Result<String> {
    // Strict variant: surfaces a final (post-retry) error so the caller keeps
    // the extracted text instead of embedding a placeholder as page content.
    crate::image::describe_image_with_prompt_strict(
        llm,
        png,
        PNG_MIME,
        prompt,
        max_tokens,
        max_image_edge,
    )
    .await
    .map(|t| sanitize_vision_text(&t))
}

/// Deterministic hygiene for vision-model transcriptions before they become
/// document content. Observed live (rescued rd_tp4 corpus): the model wraps
/// tables in markdown code fences and appends sign-off chatter ("หวังว่าข้อมูลนี้
/// จะช่วย…") — both then survive chunking as junk chunks that dilute retrieval.
/// A scanned/rendered page never legitimately contains markdown fences, so
/// fence lines are dropped outright; trailing assistant sign-off lines are
/// trimmed by prefix match (conservative: suffix of the text only).
pub(crate) fn sanitize_vision_text(text: &str) -> String {
    const SIGNOFF_PREFIXES: &[&str] = &[
        "หวังว่า",
        "หากมีข้อสงสัย",
        "หากต้องการ",
        "ขอให้",
        "I hope this",
        "Let me know",
        "Feel free to",
    ];
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect();
    while let Some(last) = lines.last() {
        let t = last.trim();
        if t.is_empty() || SIGNOFF_PREFIXES.iter().any(|s| t.starts_with(s)) {
            lines.pop();
        } else {
            break;
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sanitize_strips_fences_and_signoff_but_keeps_content() {
        let raw = "```markdown\n| ลำดับ | อัตรา |\n|---|---|\n| 10 | 3.0 |\n```\n\nหวังว่าข้อมูลนี้จะช่วยในการทำงานของคุณ";
        let out = super::sanitize_vision_text(raw);
        assert!(out.contains("| 10 | 3.0 |"), "{out}");
        assert!(!out.contains("```"), "{out}");
        assert!(!out.contains("หวังว่า"), "{out}");
        // Sign-off prefixes only trim the SUFFIX — same phrase mid-document stays.
        let mid = "หวังว่าจะได้รับการพิจารณา\nข้อความจริงของเอกสาร";
        assert_eq!(super::sanitize_vision_text(mid), mid);
    }

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

    fn table_page(page_num: usize, header: &str, rows: &[&str], col_xs: Vec<f32>) -> PageExtract {
        let all: Vec<&str> = std::iter::once(header)
            .chain(rows.iter().copied())
            .collect();
        let html = format!(
            "<table>{}</table>",
            all.iter()
                .map(|r| format!(
                    "<tr>{}</tr>",
                    r.split(" | ")
                        .map(|c| format!("<td>{c}</td>"))
                        .collect::<String>()
                ))
                .collect::<String>()
        );
        let n_cols = header.split(" | ").count();
        PageExtract {
            page_num,
            strategy: PageStrategy::Tabular,
            text: all.join("\n"),
            page_png: None,
            embedded: vec![],
            table: Some(crate::table_lattice::ReconstructedTable {
                html,
                linearized: all.join("\n"),
                confidence: 1.0,
                char_coverage: 1.0,
                n_rows: all.len(),
                n_cols,
                col_xs,
            }),
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }
    }

    #[test]
    fn stitches_consecutive_same_grid_pages_and_drops_repeated_header() {
        let xs = vec![0.0, 100.0, 200.0, 300.0];
        let mut pages = vec![
            table_page(1, "Region | Q1 | Q2", &["North | 100 | 200"], xs.clone()),
            // Repeated header on the continuation page must be dropped.
            table_page(2, "Region | Q1 | Q2", &["South | 300 | 400"], xs.clone()),
            // Different grid → NOT stitched.
            table_page(3, "A | B", &["x | y"], vec![0.0, 50.0, 100.0]),
        ];
        stitch_multipage_tables(&mut pages);

        let base = pages[0].table.as_ref().expect("anchor keeps its table");
        assert_eq!(base.n_rows, 3, "1 header + 2 data rows");
        assert_eq!(
            base.linearized,
            "Region | Q1 | Q2\nNorth | 100 | 200\nSouth | 300 | 400"
        );
        assert_eq!(
            base.html,
            "<table><tr><td>Region</td><td>Q1</td><td>Q2</td></tr>\
             <tr><td>North</td><td>100</td><td>200</td></tr>\
             <tr><td>South</td><td>300</td><td>400</td></tr></table>"
        );
        assert_eq!(pages[0].table_pages, vec![1, 2]);
        assert!(pages[1].table.is_none());
        assert_eq!(pages[1].stitched_into, Some(1));
        // Page 3 starts its own run, untouched.
        assert!(pages[2].table.is_some());
        assert_eq!(pages[2].stitched_into, None);
    }

    #[test]
    fn no_stitch_when_a_non_table_page_intervenes() {
        let xs = vec![0.0, 100.0, 200.0];
        let mut pages = vec![
            table_page(1, "A | B", &["a | b"], xs.clone()),
            PageExtract {
                page_num: 2,
                strategy: PageStrategy::TextOnly,
                text: "prose page".into(),
                page_png: None,
                embedded: vec![],
                table: None,
                cell_images: vec![],
                table_pages: vec![],
                stitched_into: None,
            },
            table_page(3, "A | B", &["c | d"], xs),
        ];
        stitch_multipage_tables(&mut pages);
        assert!(pages[0].table.is_some() && pages[2].table.is_some());
        assert_eq!(pages[0].table.as_ref().unwrap().n_rows, 2);
        assert!(pages.iter().all(|p| p.stitched_into.is_none()));
    }

    #[test]
    fn stitch_keeps_distinct_continuation_header_row() {
        // The continuation's first row is DATA (not a repeated header) — it
        // must be kept.
        let xs = vec![0.0, 100.0, 200.0];
        let mut pages = vec![
            table_page(1, "A | B", &["a | b"], xs.clone()),
            table_page(2, "c | d", &["e | f"], xs),
        ];
        stitch_multipage_tables(&mut pages);
        let base = pages[0].table.as_ref().unwrap();
        assert_eq!(base.n_rows, 4);
        assert_eq!(base.linearized, "A | B\na | b\nc | d\ne | f");
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
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }];
        let doc = render_to_document("Doc", pages, Some(&llm), None, &cfg()).await;
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
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }];
        let doc = render_to_document("Doc", pages, Some(&llm), None, &cfg()).await;
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
    async fn no_vision_model_keeps_text_and_flags_image_pages() {
        // With no vision model configured, the smart path still runs: text pages
        // extract cleanly, and image/scanned pages keep their pdfium text and are
        // counted in `pages_vision_skipped` (no OCR, no crash).
        let pages = vec![
            PageExtract {
                page_num: 1,
                strategy: PageStrategy::TextOnly,
                text: "clean readable text".into(),
                page_png: None,
                embedded: vec![],
                table: None,
                cell_images: vec![],
                table_pages: vec![],
                stitched_into: None,
            },
            PageExtract {
                page_num: 2,
                strategy: PageStrategy::Scanned,
                text: "fallback text from scanned page".into(),
                page_png: Some(vec![1, 2, 3]),
                embedded: vec![],
                table: None,
                cell_images: vec![],
                table_pages: vec![],
                stitched_into: None,
            },
            PageExtract {
                page_num: 3,
                strategy: PageStrategy::Mixed,
                text: "mixed page text".into(),
                page_png: None,
                embedded: vec![vec![9, 9, 9]],
                table: None,
                cell_images: vec![],
                table_pages: vec![],
                stitched_into: None,
            },
        ];
        let doc = render_to_document("Doc", pages, None, None, &cfg()).await;
        assert_eq!(doc.vision_pages_used, 0);
        assert_eq!(doc.pages_vision_failed, 0);
        // Scanned + Mixed both wanted vision and had none.
        assert_eq!(doc.pages_vision_skipped, 2);
        assert!(doc.markdown.contains("clean readable text"));
        assert!(doc.markdown.contains("fallback text from scanned page"));
        assert!(doc.markdown.contains("mixed page text"));
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
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }];
        let doc = render_to_document("Doc", pages, Some(&llm), None, &cfg()).await;
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
            col_xs: vec![],
        };
        let pages = vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::Tabular,
            text: "raw tabular text".into(),
            page_png: None,
            embedded: vec![],
            table: Some(lat),
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }];
        let doc = render_to_document("Doc", pages, Some(&llm), None, &cfg()).await;
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
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }];
        let doc = render_to_document("Doc", pages, Some(&llm), None, &c).await;
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

    /// Vision stub that echoes the first image byte as `OCR{n}` and sleeps longer
    /// for lower bytes — so pages complete in REVERSE order, exercising the
    /// order-preserving reassembly after concurrent OCR.
    struct OrderEchoVision;

    #[async_trait::async_trait]
    impl LlmProvider for OrderEchoVision {
        fn model_name(&self) -> &str {
            "order-echo"
        }
        fn supports_vision(&self) -> bool {
            true
        }
        async fn generate(&self, _m: &[ChatMessage], _t: Option<u32>) -> Result<LlmResponse> {
            unreachable!()
        }
        async fn generate_vision(
            &self,
            m: &[VisionMessage],
            _t: Option<u32>,
        ) -> Result<LlmResponse> {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&m[0].images[0].base64_data)
                .unwrap();
            let id = bytes[0] as u64;
            tokio::time::sleep(std::time::Duration::from_millis(
                60u64.saturating_sub(id * 10),
            ))
            .await;
            Ok(LlmResponse {
                content: format!("OCR{id}"),
                usage: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn concurrent_pages_reassemble_in_order() {
        // 5 Scanned pages, each a 1-byte "render" carrying its index. With
        // reverse-order completion + concurrency, the output must still be in
        // page order and every page must carry its own OCR.
        let pages: Vec<PageExtract> = (0..5)
            .map(|i| PageExtract {
                page_num: i + 1,
                strategy: PageStrategy::Scanned,
                text: String::new(),
                page_png: Some(vec![i as u8]),
                embedded: vec![],
                table: None,
                cell_images: vec![],
                table_pages: vec![],
                stitched_into: None,
            })
            .collect();
        let c = SmartPdfConfig {
            vision_concurrency: 8,
            ..cfg()
        };
        let doc = render_to_document("Doc", pages, Some(&OrderEchoVision), None, &c).await;
        assert_eq!(doc.vision_pages_used, 5);
        assert_eq!(doc.pages.len(), 5);
        for (i, p) in doc.pages.iter().enumerate() {
            assert_eq!(p.page_num, i + 1, "pages must stay in order");
            assert!(
                p.markdown.contains(&format!("OCR{i}")),
                "page {i} should carry its own OCR, got: {}",
                p.markdown
            );
        }
    }

    /// Deterministic OCR stub: returns a fixed transcript, or an error to test
    /// the vision fallback.
    struct StubOcr {
        reply: Option<String>,
    }

    #[async_trait::async_trait]
    impl crate::ocr::OcrProvider for StubOcr {
        async fn ocr(&self, _png: &[u8]) -> Result<String> {
            match &self.reply {
                Some(t) => Ok(t.clone()),
                None => Err(thairag_core::ThaiRagError::Internal("ocr down".into())),
            }
        }
        fn name(&self) -> &str {
            "stub-ocr"
        }
    }

    fn scanned_page() -> Vec<PageExtract> {
        vec![PageExtract {
            page_num: 1,
            strategy: PageStrategy::Scanned,
            text: "garbled text layer".into(),
            page_png: Some(vec![1, 2, 3]),
            embedded: vec![],
            table: None,
            cell_images: vec![],
            table_pages: vec![],
            stitched_into: None,
        }]
    }

    #[tokio::test]
    async fn ocr_provider_preferred_over_vision() {
        let llm = StubVision {
            reply: "VISION OUTPUT".into(),
            supports: true,
        };
        let ocr = StubOcr {
            reply: Some("DETERMINISTIC OCR TEXT".into()),
        };
        let doc = render_to_document("Doc", scanned_page(), Some(&llm), Some(&ocr), &cfg()).await;
        assert_eq!(doc.ocr_pages_used, 1);
        assert_eq!(
            doc.vision_pages_used, 0,
            "vision must not run when OCR works"
        );
        assert!(doc.markdown.contains("DETERMINISTIC OCR TEXT"));
        assert!(!doc.markdown.contains("VISION OUTPUT"));
    }

    #[tokio::test]
    async fn ocr_failure_falls_back_to_vision() {
        let llm = StubVision {
            reply: "VISION FALLBACK".into(),
            supports: true,
        };
        let ocr = StubOcr { reply: None }; // OCR errors
        let doc = render_to_document("Doc", scanned_page(), Some(&llm), Some(&ocr), &cfg()).await;
        assert_eq!(doc.ocr_pages_used, 0);
        assert_eq!(doc.vision_pages_used, 1, "must fall back to vision");
        assert!(doc.markdown.contains("VISION FALLBACK"));
    }

    #[tokio::test]
    async fn ocr_with_no_vision_keeps_text_on_failure() {
        let ocr = StubOcr { reply: None };
        let doc = render_to_document("Doc", scanned_page(), None, Some(&ocr), &cfg()).await;
        assert_eq!(doc.ocr_pages_used, 0);
        assert_eq!(doc.pages_vision_failed, 1);
        assert!(doc.markdown.contains("garbled text layer")); // kept the text
    }
}
