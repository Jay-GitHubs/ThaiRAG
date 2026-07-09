//! Regression fixture for comb-column demotion on the real RD ภ.ง.ด. form
//! (the measured table-accuracy bottleneck): before the long-segment anchor
//! gate, page 1 reconstructed as a 22-column grid (17pt comb boxes became
//! columns, confidence 0.52) and entry values smeared across misaligned
//! cells. The true structure is label + 4 entries.
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn rd_tp4_page1_reconstructs_true_five_column_grid() {
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/thai-real/rd_tp4_table.pdf"
    ))
    .expect("fixture readable");
    let pages = extract_pages(&raw, &SmartPdfConfig::default()).expect("extract");
    let p1 = pages
        .iter()
        .find(|p| p.page_num == 1)
        .and_then(|p| p.table.as_ref())
        .expect("page 1 must reconstruct as a table");

    assert_eq!(p1.n_cols, 5, "label + 4 entry columns, not comb boxes");
    assert!(
        p1.confidence >= 0.9,
        "well-formed grid fills its cells (conf={})",
        p1.confidence
    );
    // The entry-number row must carry all four entries, one per column.
    let id_row = p1
        .linearized
        .lines()
        .find(|l| l.contains("ล\u{e4d}าด\u{e31}บ") || l.contains("ลำดับ"))
        .expect("identifier row present");
    for n in ["1.", "2.", "3.", "4."] {
        assert!(
            id_row.split(" | ").any(|c| c.trim() == n),
            "entry {n} must occupy its own cell: {id_row}"
        );
    }
}
