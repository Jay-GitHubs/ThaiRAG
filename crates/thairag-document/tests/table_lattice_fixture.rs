//! End-to-end check of deterministic lattice table reconstruction against a
//! real bilingual Thai/English bordered-table PDF, through the full
//! `extract_pages` routing (no LLM, no vision). Proves a bordered table that
//! the whitespace heuristic mislabels `TextOnly` is still reconstructed from
//! geometry, with Thai text in correct logical order and exact cell content.
//! Skips cleanly when libpdfium is unavailable.

use thairag_document::pdfium_engine;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn lattice_reconstructs_real_thai_table_fixture() {
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — skipping lattice fixture check");
        return;
    }
    let pdf = std::fs::read("../../tests/fixtures/micro_sme_prohibited_business.pdf")
        .expect("fixture PDF present");
    let pages = extract_pages(&pdf, &SmartPdfConfig::default()).expect("extract_pages");

    // The bordered table page must be reconstructed by lattice even though the
    // text heuristic classifies it TextOnly.
    let lat = pages
        .iter()
        .find_map(|p| p.lattice.as_ref())
        .expect("a page should reconstruct as a lattice table");

    // Structure: a 4-column table with many rows, high confidence.
    assert_eq!(lat.n_cols, 4, "expected 4 columns, got {}", lat.n_cols);
    assert!(lat.n_rows >= 10, "expected many rows, got {}", lat.n_rows);
    assert!(lat.confidence > 0.8, "low confidence: {}", lat.confidence);
    assert!(
        lat.char_coverage > 0.8,
        "low coverage: {}",
        lat.char_coverage
    );

    // Thai header text in correct logical order (combining marks intact).
    assert!(
        lat.html.contains("ประเภทธุรกิจ"),
        "Thai header garbled/missing in: {}",
        &lat.html.chars().take(200).collect::<String>()
    );
    // English preserved, and the literal '&' is HTML-escaped (XSS-safe).
    assert!(
        lat.html.contains("Weapons &amp; munitions"),
        "english/escaping wrong"
    );
    // Output is a real HTML table (the LLM-facing payload), not markdown.
    assert!(lat.html.starts_with("<table>"));
    // Linearized embedding text is non-empty and pipe-delimited.
    assert!(lat.linearized.contains(" | "));
}
