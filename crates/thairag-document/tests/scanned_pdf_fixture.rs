//! Scoping guard for the OCR case.
//!
//! A genuinely scanned (image-only, no text layer) Thai PDF — here a 1943 Royal
//! Gazette page — must NOT yield a deterministic table: there are no glyph
//! coordinates to reconstruct from, so fabricating a grid would be wrong. The
//! pipeline must instead recognise the pages as scanned/image and route them to
//! the vision/OCR path. This locks in that boundary between the deterministic
//! path (digital text layer) and the OCR path (image-only). Skips cleanly when
//! libpdfium or the fixture is unavailable.

use thairag_document::pdfium_engine;
use thairag_document::semantic::PageStrategy;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn scanned_pdf_has_no_text_layer_and_is_routed_to_ocr() {
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — skipping");
        return;
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/thai-real/scanned_gazette_2486.pdf"
    );
    let Ok(pdf) = std::fs::read(path) else {
        eprintln!("fixture missing — skipping");
        return;
    };

    let pages = extract_pages(&pdf, &SmartPdfConfig::default()).expect("extract_pages");
    assert!(!pages.is_empty(), "expected at least one page");

    for p in &pages {
        // No text layer: pdfium extracts (effectively) nothing from the image.
        assert!(
            p.text.trim().is_empty(),
            "page {} unexpectedly had a text layer: {:?}",
            p.page_num,
            &p.text.chars().take(40).collect::<String>()
        );
        // The deterministic path must not fabricate a table from an image.
        assert!(
            p.table.is_none(),
            "page {} fabricated a table from a scanned image",
            p.page_num
        );
        // Recognised as scanned/image → routed to the vision/OCR path.
        assert!(
            matches!(p.strategy, PageStrategy::Scanned | PageStrategy::ImageHeavy),
            "page {} not routed to OCR (strategy {:?})",
            p.page_num,
            p.strategy
        );
    }
}
