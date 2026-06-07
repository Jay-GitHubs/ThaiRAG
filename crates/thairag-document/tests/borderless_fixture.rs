//! Integration check of borderless (stream) table reconstruction through the
//! full `extract_pages` path on a real PDF whose table is laid out purely by
//! column whitespace (no ruling lines). Skips cleanly when libpdfium or the
//! fixture is unavailable.

use thairag_document::pdfium_engine;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn reconstructs_borderless_table_fixture() {
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — skipping");
        return;
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/borderless_table.pdf"
    );
    let Ok(pdf) = std::fs::read(path) else {
        eprintln!("fixture missing — run `cargo run --example make_borderless_pdf`");
        return;
    };
    let pages = extract_pages(&pdf, &SmartPdfConfig::default()).expect("extract_pages");
    let t = pages
        .iter()
        .find_map(|p| p.table.as_ref())
        .expect("borderless table should be reconstructed");

    eprintln!(
        "grid {}x{} conf={:.2} cov={:.2}",
        t.n_rows, t.n_cols, t.confidence, t.char_coverage
    );
    assert!(t.n_cols >= 3, "expected >=3 columns, got {}", t.n_cols);
    assert!(t.n_rows >= 6, "expected many rows, got {}", t.n_rows);
    // Exact cell content (from the text layer); intra-cell space preserved.
    assert!(t.html.contains("<td>North</td>"), "{}", t.html);
    assert!(t.html.contains("<td>200</td>"), "{}", t.html);
    assert!(
        t.html.contains("Q1 Sales"),
        "intra-cell space lost: {}",
        t.html
    );
    // The one-line title is excluded from the table region (not a cell).
    assert!(
        !t.html.contains("Quarterly Sales"),
        "title leaked into table: {}",
        t.html
    );
}
