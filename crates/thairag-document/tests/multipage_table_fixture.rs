//! End-to-end check of multi-page table stitching against the
//! `multipage_table` edge-case fixture (same 3-column grid on two pages, the
//! header repeated on page 2), through the full `extract_pages` routing (no
//! LLM, no vision): the two page tables merge into one 5-row table on page 1
//! with the repeated header dropped, and page 2 is marked as stitched. Skips
//! cleanly when libpdfium is unavailable.

use thairag_document::pdfium_engine;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn consecutive_same_grid_pages_stitch_into_one_table() {
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — skipping multipage fixture check");
        return;
    }
    let pdf = std::fs::read("../../tests/fixtures/edge-cases/multipage_table.pdf")
        .expect("fixture PDF present");
    // The minimal fixture has ~38 glyphs per page; lower the readable-page
    // floor so the text-layer gate lets the lattice run.
    let cfg = SmartPdfConfig {
        min_chars_per_page: 5,
        ..Default::default()
    };
    let pages = extract_pages(&pdf, &cfg).expect("extract_pages");
    assert_eq!(pages.len(), 2);

    let base = pages[0].table.as_ref().expect("anchor table on page 1");
    assert_eq!(
        (base.n_rows, base.n_cols),
        (5, 3),
        "header + 4 data rows (repeated header dropped), got {}x{}",
        base.n_rows,
        base.n_cols
    );
    // One header, all four regions, in reading order.
    assert_eq!(base.linearized.matches("Region").count(), 1);
    for row in [
        "North | 100 | 200",
        "South | 300 | 400",
        "East | 500 | 600",
        "West | 700 | 800",
    ] {
        assert!(
            base.linearized.contains(row),
            "missing row {row:?} in: {}",
            base.linearized
        );
    }
    // One merged HTML table, not two concatenated ones.
    assert_eq!(base.html.matches("<table>").count(), 1);
    assert_eq!(base.html.matches("</table>").count(), 1);
    assert_eq!(pages[0].table_pages, vec![1, 2]);

    // The continuation page carries no duplicate content.
    assert!(pages[1].table.is_none());
    assert_eq!(pages[1].stitched_into, Some(1));
}
