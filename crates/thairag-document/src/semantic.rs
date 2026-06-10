//! Per-page strategy selection and semantic-markdown assembly for the smart
//! document-extraction pipeline.
//!
//! Ported from `Jay-RAG-Tools/crates/core/src/processor.rs`. The goal is to
//! turn an uploaded document into **one canonical semantic-markdown document**
//! — body text, markdown tables, and image descriptions interleaved in reading
//! order — that is ideal for an AI to consume before chunking.
//!
//! This module holds the *pure* decision and assembly logic (no pdfium / IO),
//! so it is fully unit-testable. The native page extraction (a `pdfium_engine`
//! module) and the async vision calls are wired in by the pipeline.

/// How a single page should be turned into semantic markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStrategy {
    /// Extractable text, no significant images, not tabular — use text as-is.
    TextOnly,
    /// Body text plus embedded images — keep text, describe each image.
    Mixed,
    /// Large image(s) dominate but readable text remains — render the whole
    /// page and OCR/describe it via the vision model.
    ImageHeavy,
    /// Little/no extractable text and image-dominated — a scanned page; render
    /// and OCR the whole page via the vision model.
    Scanned,
    /// Readable text that looks tabular — render the page and have the vision
    /// model emit a markdown table.
    Tabular,
}

impl PageStrategy {
    /// Stable string for `ChunkMetadata.page_strategy` + telemetry.
    pub fn as_str(self) -> &'static str {
        match self {
            PageStrategy::TextOnly => "pdf_text_only",
            PageStrategy::Mixed => "pdf_mixed",
            PageStrategy::ImageHeavy => "pdf_image_heavy",
            PageStrategy::Scanned => "pdf_scanned",
            PageStrategy::Tabular => "pdf_tabular",
        }
    }

    /// Whether this strategy requires a vision-LLM call (render + describe).
    /// `TextOnly` is the only purely mechanical path.
    pub fn needs_vision(self) -> bool {
        !matches!(self, PageStrategy::TextOnly)
    }
}

/// Thresholds for [`select_page_strategy`]. Defaults mirror Jay-RAG-Tools.
#[derive(Debug, Clone, Copy)]
pub struct StrategyThresholds {
    /// Image-coverage ratio (0.0..=1.0) at/above which a page is image-heavy.
    pub page_as_image_threshold: f64,
    /// Minimum meaningful chars for a page's text to count as "readable".
    pub min_chars_per_page: usize,
}

impl Default for StrategyThresholds {
    fn default() -> Self {
        Self {
            page_as_image_threshold: 0.5,
            min_chars_per_page: 50,
        }
    }
}

/// Pick the extraction strategy for one page from cheaply-computed signals.
///
/// Pure function (no pdfium / IO). Mirrors
/// `Jay-RAG-Tools/crates/core/src/processor.rs:238-303`, with explicit
/// `TextOnly` / `Scanned` outcomes:
///
/// | coverage ≥ thr | readable text | tabular | embedded imgs | → strategy   |
/// |----------------|---------------|---------|---------------|--------------|
/// | yes            | yes           | —       | —             | `ImageHeavy` |
/// | yes            | no            | —       | —             | `Scanned`    |
/// | no             | no            | —       | —             | `Scanned`    |
/// | no             | yes           | yes     | —             | `Tabular`    |
/// | no             | yes           | no      | >0            | `Mixed`      |
/// | no             | yes           | no      | 0             | `TextOnly`   |
///
/// - `coverage`: fraction of page area covered by image objects.
/// - `meaningful_chars`: `text_utils::meaningful_char_count` of the page text.
/// - `looks_tabular`: `table_extractor::looks_like_table` on the page text.
/// - `embedded_image_count`: number of extractable embedded images.
pub fn select_page_strategy(
    coverage: f64,
    meaningful_chars: usize,
    looks_tabular: bool,
    embedded_image_count: usize,
    thresholds: &StrategyThresholds,
) -> PageStrategy {
    let image_heavy = coverage >= thresholds.page_as_image_threshold;
    let has_text = meaningful_chars >= thresholds.min_chars_per_page;

    match (image_heavy, has_text) {
        (true, true) => PageStrategy::ImageHeavy,
        (true, false) => PageStrategy::Scanned,
        (false, false) => PageStrategy::Scanned,
        (false, true) => {
            if looks_tabular {
                PageStrategy::Tabular
            } else if embedded_image_count > 0 {
                PageStrategy::Mixed
            } else {
                PageStrategy::TextOnly
            }
        }
    }
}

/// One page's rendered semantic markdown, ready to assemble.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// 1-indexed page number, used to order the assembled document.
    pub page_num: usize,
    /// The strategy that produced this page (telemetry / debugging).
    pub strategy: PageStrategy,
    /// The page's semantic markdown body (text + tables + `[IMAGE:..]` markers).
    pub markdown: String,
    /// For a Tabular page reconstructed by deterministic lattice: the HTML table
    /// (atomic chunk payload). `None` for non-table or vision-fallback pages.
    pub table_html: Option<String>,
    /// The row-linearized form of `table_html`, used as the chunk's embedding
    /// text so retrieval matches on clean words rather than HTML tags.
    pub table_linearized: Option<String>,
    /// All page numbers whose rows live in `table_html` when a multi-page
    /// table was stitched into this page. Empty for a single-page table.
    pub table_pages: Vec<usize>,
}

/// The stable inline marker for an image blob inside the semantic markdown.
///
/// Downstream chunking keeps these intact and the admin UI resolves them to
/// `GET .../documents/{doc}/images/{id}`.
pub fn image_marker(blob_id: &str) -> String {
    format!("[IMAGE:{blob_id}]")
}

/// Assemble per-page markdown into one semantic document, in page order.
///
/// Mirrors Jay-RAG-Tools' `_enriched.md` shape: an optional document title,
/// then `## Page N` sections separated by a `---` rule. Pages are sorted by
/// `page_num` so concurrent rendering stays deterministic.
pub fn assemble_document_markdown(title: &str, mut pages: Vec<RenderedPage>) -> String {
    pages.sort_by_key(|p| p.page_num);

    let mut out = String::new();
    let title = title.trim();
    if !title.is_empty() {
        out.push_str("# ");
        out.push_str(title);
        out.push('\n');
    }

    for page in &pages {
        out.push_str("\n\n---\n## Page ");
        out.push_str(&page.page_num.to_string());
        out.push('\n');
        let body = page.markdown.trim();
        if !body.is_empty() {
            out.push_str(body);
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thr() -> StrategyThresholds {
        StrategyThresholds::default()
    }

    #[test]
    fn image_heavy_when_high_coverage_with_text() {
        assert_eq!(
            select_page_strategy(0.8, 200, false, 0, &thr()),
            PageStrategy::ImageHeavy
        );
    }

    #[test]
    fn scanned_when_high_coverage_no_text() {
        assert_eq!(
            select_page_strategy(0.9, 5, false, 0, &thr()),
            PageStrategy::Scanned
        );
    }

    #[test]
    fn scanned_when_low_coverage_no_text() {
        // An image-light page that still yields no extractable text is scanned.
        assert_eq!(
            select_page_strategy(0.1, 0, false, 0, &thr()),
            PageStrategy::Scanned
        );
    }

    #[test]
    fn tabular_beats_mixed_and_text() {
        assert_eq!(
            select_page_strategy(0.0, 300, true, 2, &thr()),
            PageStrategy::Tabular
        );
    }

    #[test]
    fn mixed_when_text_plus_embedded_images() {
        assert_eq!(
            select_page_strategy(0.2, 300, false, 3, &thr()),
            PageStrategy::Mixed
        );
    }

    #[test]
    fn text_only_when_just_text() {
        assert_eq!(
            select_page_strategy(0.0, 300, false, 0, &thr()),
            PageStrategy::TextOnly
        );
        assert!(!PageStrategy::TextOnly.needs_vision());
        assert!(PageStrategy::Tabular.needs_vision());
    }

    #[test]
    fn threshold_boundaries_are_inclusive() {
        // coverage exactly at threshold counts as image-heavy; chars exactly at
        // min counts as readable.
        assert_eq!(
            select_page_strategy(0.5, 50, false, 0, &thr()),
            PageStrategy::ImageHeavy
        );
    }

    #[test]
    fn assemble_orders_pages_and_emits_sections() {
        let pages = vec![
            RenderedPage {
                page_num: 2,
                strategy: PageStrategy::TextOnly,
                markdown: "second".into(),
                table_html: None,
                table_linearized: None,
                table_pages: vec![],
            },
            RenderedPage {
                page_num: 1,
                strategy: PageStrategy::Tabular,
                markdown: "| a | b |\n|---|---|\n| 1 | 2 |".into(),
                table_html: None,
                table_linearized: None,
                table_pages: vec![],
            },
        ];
        let md = assemble_document_markdown("My Doc", pages);
        assert!(md.starts_with("# My Doc\n"));
        let p1 = md.find("## Page 1").unwrap();
        let p2 = md.find("## Page 2").unwrap();
        assert!(p1 < p2, "pages must be ordered: {md}");
        assert!(md.contains("| a | b |"));
        assert!(md.contains("second"));
    }

    #[test]
    fn assemble_skips_empty_title_and_blank_bodies() {
        let pages = vec![RenderedPage {
            page_num: 1,
            strategy: PageStrategy::TextOnly,
            markdown: "   ".into(),
            table_html: None,
            table_linearized: None,
            table_pages: vec![],
        }];
        let md = assemble_document_markdown("  ", pages);
        assert!(!md.starts_with('#'));
        assert!(md.contains("## Page 1"));
    }

    #[test]
    fn image_marker_format_is_stable() {
        assert_eq!(image_marker("abc-123"), "[IMAGE:abc-123]");
    }

    #[test]
    fn strategy_strings_are_stable() {
        assert_eq!(PageStrategy::TextOnly.as_str(), "pdf_text_only");
        assert_eq!(PageStrategy::ImageHeavy.as_str(), "pdf_image_heavy");
        assert_eq!(PageStrategy::Tabular.as_str(), "pdf_tabular");
        assert_eq!(PageStrategy::Scanned.as_str(), "pdf_scanned");
        assert_eq!(PageStrategy::Mixed.as_str(), "pdf_mixed");
    }
}
