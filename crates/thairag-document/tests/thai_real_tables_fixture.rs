//! Real-world regression for noisy Thai government table PDFs.
//!
//! The Revenue Department withholding-tax reference (`rd_withholding_table.pdf`)
//! draws its bordered, merged-cell tables with hundreds of short per-cell border
//! segments. Before the line-clustering + coverage-gate fix, those fragments
//! exploded each table into 16–30 spurious thin columns (fill-ratio "confidence"
//! 0.07–0.17), so every page was rejected and silently dropped to flat text.
//!
//! This guards that the deterministic path now reconstructs most pages into
//! merged HTML tables with exact cell content. Skips cleanly when libpdfium or
//! the fixture is unavailable.

use thairag_document::pdfium_engine;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn real_thai_withholding_tables_reconstruct() {
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — skipping");
        return;
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/thai-real/rd_withholding_table.pdf"
    );
    let Ok(pdf) = std::fs::read(path) else {
        eprintln!("fixture missing — skipping");
        return;
    };

    let pages = extract_pages(&pdf, &SmartPdfConfig::default()).expect("extract_pages");
    let with_table = pages.iter().filter(|p| p.table.is_some()).count();
    let with_merged = pages
        .iter()
        .filter(|p| {
            p.table
                .as_ref()
                .is_some_and(|t| t.html.contains("colspan") || t.html.contains("rowspan"))
        })
        .count();
    eprintln!(
        "pages={} reconstructed={} merged={}",
        pages.len(),
        with_table,
        with_merged
    );

    // Before the fix this was 0 (all over-segmented → dropped). Now most pages
    // reconstruct, and they carry merged cells.
    assert!(
        with_table >= 10,
        "expected most pages to reconstruct a table, got {with_table}"
    );
    assert!(
        with_merged >= 10,
        "expected merged-cell tables, got {with_merged}"
    );

    // Exact cell content from the text layer survives (Thai tax-form tokens).
    let html: String = pages
        .iter()
        .filter_map(|p| p.table.as_ref())
        .map(|t| t.html.as_str())
        .collect();
    assert!(
        html.contains("ภ.ง.ด."),
        "expected Thai form-type tokens in the reconstructed tables"
    );
}
