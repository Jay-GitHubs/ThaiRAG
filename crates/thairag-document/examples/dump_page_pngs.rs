//! Render selected PDF pages to PNG files for OCR benchmarking (Phase 3 spike +
//! Phase 1b eval, see `docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`).
//!
//! Run:
//!   cargo run -p thairag-document --example dump_page_pngs -- <out_dir> <pdf> [pages]
//! `pages` is a comma list of 1-indexed pages (default: all, capped at 20).
//! Output: <out_dir>/<stem>_p<NN>.png at 200 DPI, plus <stem>_p<NN>.gt.txt — the
//! pdfium text layer for that page. For a clean (non-garbled) page the text layer
//! is trustworthy ground truth, so the CER harness can score OCR against it
//! without any manual labeling.

use std::collections::HashMap;
use std::path::Path;

use thairag_document::pdfium_engine::{self, PdfEngine};

const DPI: u32 = 200;
const MAX_PAGES: usize = 20;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: dump_page_pngs <out_dir> <pdf> [pages csv, 1-indexed]");
        return;
    }
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable");
        return;
    }
    let out_dir = &args[0];
    let pdf_path = &args[1];
    std::fs::create_dir_all(out_dir).expect("create out dir");

    let bytes = std::fs::read(pdf_path).expect("read pdf");
    let engine = PdfEngine::new().expect("pdfium");
    let total = engine.page_count(&bytes).unwrap_or(0);

    // Per-page text layer = ground truth for clean pages (the CER harness filters
    // out garbled/empty ones). Use the SAME engine — a second `PdfEngine::new()`
    // would re-bind pdfium's process-global library and hang.
    let text_by_page: HashMap<usize, String> = engine
        .page_signals(&bytes)
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.index, s.text))
        .collect();

    let pages: Vec<usize> = if args.len() >= 3 {
        args[2]
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|p| *p >= 1 && *p <= total)
            .map(|p| p - 1)
            .collect()
    } else {
        (0..total.min(MAX_PAGES)).collect()
    };

    let stem = Path::new(pdf_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();

    for pi in pages {
        match engine.render_page_png(&bytes, pi, DPI, false) {
            Ok(img) => {
                let path = format!("{out_dir}/{stem}_p{:02}.png", pi + 1);
                std::fs::write(&path, &img.png_bytes).expect("write png");
                if let Some(text) = text_by_page.get(&pi) {
                    let gt = format!("{out_dir}/{stem}_p{:02}.gt.txt", pi + 1);
                    std::fs::write(&gt, text).expect("write gt");
                }
                println!("wrote {path} ({} KB)", img.png_bytes.len() / 1024);
            }
            Err(e) => eprintln!("page {}: render failed: {e}", pi + 1),
        }
    }
}
