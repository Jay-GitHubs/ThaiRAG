//! Unified region router (Phase 2 of the complexity-routing roadmap — see
//! `docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`).
//!
//! The *decision* layer: given cheaply-computed signals for one region (a PDF
//! page, or a whole non-PDF document), pick a complexity [`RegionClass`] and the
//! [`FidelityTier`] it should be served at. This generalizes the PDF-only
//! [`select_page_strategy`](crate::semantic::select_page_strategy) across formats
//! and encodes the **fidelity ladder** + the **golden rule**:
//!
//! > Never use a probabilistic method (OCR / vision LLM) when a deterministic one
//! > is available for that region — e.g. never OCR a reconstructable table.
//!
//! This module is *pure* (no IO / pdfium / network), so it is fully unit-testable
//! and is exercised by the Phase 1 profiler. Region *execution* via handlers (and
//! the deterministic-OCR tier itself) is Phase 3 — this only decides the routing.

use crate::semantic::{PageStrategy, StrategyThresholds, select_page_strategy};

/// Source format of the document a region came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Pdf,
    Docx,
    Xlsx,
    Html,
    Image,
    Text,
    Other,
}

/// Fidelity tier a region is served at. Lower is more exact and preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidelityTier {
    /// Native structured extraction (text layer / XML / cells / lattice table).
    /// Exact, deterministic, no model.
    Native,
    /// Deterministic OCR of a rendered region (PaddleOCR). For regions with no
    /// trustworthy text layer. No hallucination; local.
    DeterministicOcr,
    /// Vision LLM — figure/diagram description, or last-resort OCR. Probabilistic.
    VisionLlm,
}

/// Complexity class for one region. Generalizes [`PageStrategy`] with the
/// text-layer-corruption case split out and non-PDF document classes added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionClass {
    /// Clean text layer, no dominant imagery, not tabular.
    NativeText,
    /// A deterministically reconstructed table (lattice/stream/native cells).
    NativeTable,
    /// Table-shaped text with no trustworthy reconstruction — raw text kept
    /// verbatim (numbers exact) rather than handed to a hallucination-prone model.
    TabularAsText,
    /// Body text plus embedded figures — keep the text, describe the figures.
    Mixed,
    /// Imagery dominates but some readable text remains.
    ImageHeavy,
    /// No usable text layer — a scanned page.
    Scanned,
    /// Text present but corrupted by a broken ToUnicode CMap (`เรืĻอง`); the text
    /// layer is untrustworthy, so OCR the rendered pixels.
    CorruptedText,
    /// A whole DOCX/XLSX/HTML document with native structured extraction.
    NativeStruct,
    /// A direct image upload.
    DirectImage,
    /// Format we don't handle.
    Unsupported,
}

impl RegionClass {
    /// Stable string for telemetry / reports.
    pub fn as_str(self) -> &'static str {
        match self {
            RegionClass::NativeText => "NativeText",
            RegionClass::NativeTable => "NativeTable",
            RegionClass::TabularAsText => "TabularAsText",
            RegionClass::Mixed => "Mixed",
            RegionClass::ImageHeavy => "ImageHeavy",
            RegionClass::Scanned => "Scanned",
            RegionClass::CorruptedText => "CorruptedText",
            RegionClass::NativeStruct => "NativeStruct",
            RegionClass::DirectImage => "DirectImage",
            RegionClass::Unsupported => "Unsupported",
        }
    }

    /// The fidelity tier this class's *primary* content is served at.
    ///
    /// Golden rule, enforced here: every class whose content comes from native
    /// structured extraction (text layer, native struct, deterministic table —
    /// including `TabularAsText`, whose verbatim text is still native) returns
    /// [`FidelityTier::Native`]. Only classes with no trustworthy text layer fall
    /// to OCR/vision.
    pub fn tier(self) -> FidelityTier {
        match self {
            RegionClass::NativeText
            | RegionClass::NativeTable
            | RegionClass::TabularAsText
            | RegionClass::NativeStruct
            | RegionClass::Mixed => FidelityTier::Native,
            RegionClass::CorruptedText | RegionClass::Scanned | RegionClass::ImageHeavy => {
                FidelityTier::DeterministicOcr
            }
            RegionClass::DirectImage => FidelityTier::VisionLlm,
            RegionClass::Unsupported => FidelityTier::Native,
        }
    }

    /// Whether the region additionally needs a vision-LLM pass to *describe*
    /// embedded figures (over and above its primary tier). True for `Mixed`,
    /// whose body text is native but whose figures carry meaning only a VLM reads.
    pub fn needs_figure_description(self) -> bool {
        matches!(self, RegionClass::Mixed)
    }

    /// Bridge from an already-selected PDF [`PageStrategy`] (plus the two
    /// refinement flags) to a [`RegionClass`]. This is the single source of truth
    /// for the PDF taxonomy — both [`classify`] and consumers that already hold a
    /// `PageStrategy` (e.g. the profiler reading `PageExtract.strategy`) route
    /// through it.
    ///
    /// `text_garbled` covers both the pre-refinement view (a `TextOnly`/`Tabular`/
    /// `Mixed` page whose text layer is corrupt) and the pdfium engine's
    /// post-refinement view (such a page is upgraded to `Scanned` but keeps its
    /// corrupt text). `ImageHeavy` is excluded — it already routes to vision.
    pub fn from_page_strategy(
        base: PageStrategy,
        text_garbled: bool,
        has_deterministic_table: bool,
    ) -> RegionClass {
        // A successful deterministic table wins for ANY strategy: render_to_document
        // renders from the lattice HTML before consulting the page strategy (exact
        // cells from the text layer — the golden rule). The pdfium engine only ever
        // reconstructs a table on a non-scanned, non-garbled page, so this can't
        // mask a genuine scan or a corrupt text layer.
        if has_deterministic_table {
            return RegionClass::NativeTable;
        }
        if text_garbled && !matches!(base, PageStrategy::ImageHeavy) {
            return RegionClass::CorruptedText;
        }
        match base {
            PageStrategy::TextOnly => RegionClass::NativeText,
            PageStrategy::Tabular => RegionClass::TabularAsText,
            PageStrategy::Mixed => RegionClass::Mixed,
            PageStrategy::ImageHeavy => RegionClass::ImageHeavy,
            PageStrategy::Scanned => RegionClass::Scanned,
        }
    }
}

/// Cheaply-computed signals for one region. Format-agnostic; the PDF-specific
/// signals are simply left at their defaults for non-PDF formats.
#[derive(Debug, Clone)]
pub struct RegionSignals {
    pub format: SourceFormat,
    /// Fraction of region area covered by image objects (PDF pages).
    pub image_coverage: f64,
    /// `text_utils::meaningful_char_count` of the region text.
    pub meaningful_chars: usize,
    /// `table_extractor::looks_like_table` on the region text.
    pub looks_tabular: bool,
    /// Number of extractable embedded images.
    pub embedded_image_count: usize,
    /// `text_utils::text_layer_garbled` — the text layer is corrupted.
    pub text_layer_garbled: bool,
    /// A deterministic table reconstruction succeeded for this region.
    pub has_deterministic_table: bool,
    pub thresholds: StrategyThresholds,
}

impl RegionSignals {
    /// PDF page signals with default thresholds.
    pub fn pdf_page(
        image_coverage: f64,
        meaningful_chars: usize,
        looks_tabular: bool,
        embedded_image_count: usize,
        text_layer_garbled: bool,
        has_deterministic_table: bool,
    ) -> Self {
        Self {
            format: SourceFormat::Pdf,
            image_coverage,
            meaningful_chars,
            looks_tabular,
            embedded_image_count,
            text_layer_garbled,
            has_deterministic_table,
            thresholds: StrategyThresholds::default(),
        }
    }

    /// Document-level signals for a non-PDF format.
    pub fn document(format: SourceFormat) -> Self {
        Self {
            format,
            image_coverage: 0.0,
            meaningful_chars: 0,
            looks_tabular: false,
            embedded_image_count: 0,
            text_layer_garbled: false,
            has_deterministic_table: false,
            thresholds: StrategyThresholds::default(),
        }
    }
}

/// The routing decision for a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionPlan {
    pub class: RegionClass,
    pub tier: FidelityTier,
    pub needs_figure_description: bool,
}

/// Classify a region into its complexity class.
///
/// For PDF pages this is faithful to the live pipeline: it reproduces
/// [`select_page_strategy`] exactly, then applies the same two refinements the
/// pdfium engine applies — a successful deterministic table wins (`NativeTable`),
/// and a non-vision page whose text layer is garbled is routed to OCR
/// (`CorruptedText`) instead of trusting the corrupt text.
pub fn classify(sig: &RegionSignals) -> RegionClass {
    match sig.format {
        SourceFormat::Docx | SourceFormat::Xlsx | SourceFormat::Html => {
            return RegionClass::NativeStruct;
        }
        SourceFormat::Image => return RegionClass::DirectImage,
        SourceFormat::Text => return RegionClass::NativeText,
        SourceFormat::Other => return RegionClass::Unsupported,
        SourceFormat::Pdf => {}
    }

    let base = select_page_strategy(
        sig.image_coverage,
        sig.meaningful_chars,
        sig.looks_tabular,
        sig.embedded_image_count,
        &sig.thresholds,
    );
    RegionClass::from_page_strategy(base, sig.text_layer_garbled, sig.has_deterministic_table)
}

/// Full routing plan for a region: class + fidelity tier + figure-description need.
pub fn plan(sig: &RegionSignals) -> RegionPlan {
    let class = classify(sig);
    RegionPlan {
        class,
        tier: class.tier(),
        needs_figure_description: class.needs_figure_description(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thr() -> StrategyThresholds {
        StrategyThresholds::default()
    }

    /// The router must reproduce `select_page_strategy` exactly for PDF pages
    /// with no table and no corruption — over the whole signal grid.
    #[test]
    fn faithful_to_select_page_strategy() {
        let coverages = [0.0, 0.3, 0.5, 0.9];
        let chars = [0usize, 49, 50, 500];
        for &cov in &coverages {
            for &ch in &chars {
                for &tab in &[false, true] {
                    for &emb in &[0usize, 3] {
                        let base = select_page_strategy(cov, ch, tab, emb, &thr());
                        let sig = RegionSignals::pdf_page(cov, ch, tab, emb, false, false);
                        let got = classify(&sig);
                        let expect = match base {
                            PageStrategy::TextOnly => RegionClass::NativeText,
                            PageStrategy::Tabular => RegionClass::TabularAsText,
                            PageStrategy::Mixed => RegionClass::Mixed,
                            PageStrategy::ImageHeavy => RegionClass::ImageHeavy,
                            PageStrategy::Scanned => RegionClass::Scanned,
                        };
                        assert_eq!(got, expect, "cov={cov} ch={ch} tab={tab} emb={emb}");
                    }
                }
            }
        }
    }

    #[test]
    fn deterministic_table_wins_and_stays_native() {
        // A readable tabular page with a reconstructed table → NativeTable, tier 1.
        let sig = RegionSignals::pdf_page(0.1, 800, true, 0, false, true);
        assert_eq!(classify(&sig), RegionClass::NativeTable);
        assert_eq!(classify(&sig).tier(), FidelityTier::Native);
    }

    #[test]
    fn golden_rule_no_native_table_ever_ocrd() {
        // Whatever the other signals, a region with a deterministic table on a
        // non-scanned page is served natively — never OCR/vision.
        for &cov in &[0.0, 0.4] {
            for &ch in &[100usize, 1000] {
                for &tab in &[false, true] {
                    for &emb in &[0usize, 2] {
                        let sig = RegionSignals::pdf_page(cov, ch, tab, emb, false, true);
                        let plan = plan(&sig);
                        if plan.class == RegionClass::NativeTable {
                            assert_eq!(plan.tier, FidelityTier::Native);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn garbled_text_routes_to_ocr_not_native() {
        // A page the strategy would read as TextOnly, but garbled → CorruptedText
        // → deterministic OCR tier (never trust the corrupt text layer).
        let sig = RegionSignals::pdf_page(0.1, 800, false, 0, true, false);
        assert_eq!(classify(&sig), RegionClass::CorruptedText);
        assert_eq!(classify(&sig).tier(), FidelityTier::DeterministicOcr);
    }

    #[test]
    fn genuine_scan_and_imageheavy_route_to_ocr() {
        // A genuine scan has no text layer, so it can't be flagged garbled → stays
        // Scanned (the garble flag only fires on pages that DO have Thai text).
        let scan = RegionSignals::pdf_page(0.9, 0, false, 0, false, false);
        assert_eq!(classify(&scan), RegionClass::Scanned);
        // An ImageHeavy page (high coverage + readable text) is already OCR-bound,
        // so a garble flag doesn't reclassify it — it stays ImageHeavy.
        let ih = RegionSignals::pdf_page(0.9, 800, false, 0, true, false);
        assert_eq!(classify(&ih), RegionClass::ImageHeavy);
    }

    #[test]
    fn deterministic_table_wins_over_imageheavy() {
        // render_to_document renders a reconstructed table for ANY strategy, so an
        // ImageHeavy page that also reconstructed a table is NativeTable, not OCR.
        let sig = RegionSignals::pdf_page(0.9, 800, false, 0, false, true);
        assert_eq!(classify(&sig), RegionClass::NativeTable);
        assert_eq!(classify(&sig).tier(), FidelityTier::Native);
    }

    #[test]
    fn clean_text_layer_is_native_text() {
        let sig = RegionSignals::pdf_page(0.1, 800, false, 0, false, false);
        let plan = plan(&sig);
        assert_eq!(plan.class, RegionClass::NativeText);
        assert_eq!(plan.tier, FidelityTier::Native);
        assert!(!plan.needs_figure_description);
    }

    #[test]
    fn mixed_is_native_text_plus_figure_description() {
        let sig = RegionSignals::pdf_page(0.2, 800, false, 3, false, false);
        let plan = plan(&sig);
        assert_eq!(plan.class, RegionClass::Mixed);
        assert_eq!(plan.tier, FidelityTier::Native); // text is native
        assert!(plan.needs_figure_description); // figures need a VLM pass
    }

    #[test]
    fn bridge_handles_post_upgrade_scanned_garble() {
        // The pdfium engine upgrades a garbled TextOnly/Tabular/Mixed page to
        // Scanned but keeps its corrupt text; the profiler reads that Scanned
        // strategy + the garble flag and must recover CorruptedText.
        assert_eq!(
            RegionClass::from_page_strategy(PageStrategy::Scanned, true, false),
            RegionClass::CorruptedText
        );
        // A genuine scan (no garble) stays Scanned.
        assert_eq!(
            RegionClass::from_page_strategy(PageStrategy::Scanned, false, false),
            RegionClass::Scanned
        );
        // Clean tabular page with a reconstruction → NativeTable.
        assert_eq!(
            RegionClass::from_page_strategy(PageStrategy::Tabular, false, true),
            RegionClass::NativeTable
        );
    }

    #[test]
    fn non_pdf_formats_route_by_format() {
        assert_eq!(
            classify(&RegionSignals::document(SourceFormat::Docx)),
            RegionClass::NativeStruct
        );
        assert_eq!(
            classify(&RegionSignals::document(SourceFormat::Xlsx)),
            RegionClass::NativeStruct
        );
        assert_eq!(
            classify(&RegionSignals::document(SourceFormat::Html)),
            RegionClass::NativeStruct
        );
        assert_eq!(
            classify(&RegionSignals::document(SourceFormat::Image)),
            RegionClass::DirectImage
        );
        assert_eq!(
            classify(&RegionSignals::document(SourceFormat::Image)).tier(),
            FidelityTier::VisionLlm
        );
        assert_eq!(
            classify(&RegionSignals::document(SourceFormat::Text)),
            RegionClass::NativeText
        );
    }
}
