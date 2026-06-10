//! End-to-end check of in-cell image capture against the `images_in_cells`
//! edge-case fixture, through the full `extract_pages` routing (no LLM, no
//! vision): each image lands in its containing cell as an `[IMAGE:<id>]`
//! marker, and the PNG bytes ride along as `cell_images` blobs whose ids match
//! the markers. Skips cleanly when libpdfium is unavailable.

use thairag_document::pdfium_engine;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};

#[test]
fn cell_images_become_markers_and_blobs() {
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — skipping images-in-cells fixture check");
        return;
    }
    let pdf = std::fs::read("../../tests/fixtures/edge-cases/images_in_cells.pdf")
        .expect("fixture PDF present");
    // The minimal fixture has only 12 glyphs ("Logo A"/"Logo B"); lower the
    // readable-page floor so the text-layer gate lets the lattice run.
    let cfg = SmartPdfConfig {
        min_chars_per_page: 5,
        ..Default::default()
    };
    let pages = extract_pages(&pdf, &cfg).expect("extract_pages");

    let page = pages
        .iter()
        .find(|p| p.table.is_some())
        .expect("the table page should reconstruct");
    let lat = page.table.as_ref().unwrap();

    assert_eq!((lat.n_rows, lat.n_cols), (2, 2));
    assert_eq!(page.cell_images.len(), 2, "both cell images captured");
    for ci in &page.cell_images {
        let marker = format!("[IMAGE:{}]", ci.image_id);
        assert!(
            lat.html.contains(&marker) && lat.linearized.contains(&marker),
            "marker {marker} missing from table output: {}",
            lat.linearized
        );
        assert!(!ci.png.is_empty(), "blob bytes present");
        assert_eq!((ci.width, ci.height), (48, 48), "raw logo dimensions");
    }
    // Image-only cells count as filled — the grid is fully confident.
    assert!(lat.confidence > 0.99, "conf: {}", lat.confidence);
    // Text cells unaffected.
    assert!(lat.linearized.contains("Logo A") && lat.linearized.contains("Logo B"));
}
