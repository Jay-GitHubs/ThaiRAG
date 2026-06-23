//! Render selected PDF pages to PNG files for OCR benchmarking (Phase 3 spike —
//! deterministic OCR vs vision LLM, see `docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`).
//!
//! Run:
//!   cargo run -p thairag-document --example dump_page_pngs -- <out_dir> <pdf> [pages]
//! `pages` is a comma list of 1-indexed pages (default: all, capped at 20).
//! Output files: <out_dir>/<stem>_p<NN>.png at 200 DPI.

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
                println!("wrote {path} ({} KB)", img.png_bytes.len() / 1024);
            }
            Err(e) => eprintln!("page {}: render failed: {e}", pi + 1),
        }
    }
}
