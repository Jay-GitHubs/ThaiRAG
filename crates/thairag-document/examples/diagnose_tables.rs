//! Gap-report diagnostic: run PDFs through pdfium geometry + deterministic table
//! reconstruction (lattice → stream) and print a per-page report. No vision, no
//! AI, no DB — purely shows what the deterministic path recovers, so we can see
//! which real docs are deterministic-OK vs genuinely need ML/OCR.
//!
//! Run: cargo run -p thairag-document --example diagnose_tables -- <pdf>...

use thairag_document::pdfium_engine::{self, PdfEngine};
use thairag_document::{table_lattice, table_stream};

const MAX_PAGES: usize = 15;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: diagnose_tables <pdf>...");
        return;
    }
    if !pdfium_engine::is_available() {
        eprintln!("libpdfium unavailable — cannot run");
        return;
    }
    let engine = PdfEngine::new().expect("init pdfium engine");

    for path in &args {
        let Ok(pdf) = std::fs::read(path) else {
            println!("\n=== {path} === (unreadable, skipped)");
            continue;
        };
        let pages = engine.page_count(&pdf).unwrap_or(0);
        println!(
            "\n=== {} === ({} pages, {} KB){}",
            path,
            pages,
            pdf.len() / 1024,
            if pages > MAX_PAGES {
                format!(" — showing first {MAX_PAGES}")
            } else {
                String::new()
            }
        );

        let mut text_pages = 0usize;
        let mut table_pages = 0usize;
        let mut merged_pages = 0usize;
        for p in 0..pages.min(MAX_PAGES) {
            let g = match engine.page_geometry(&pdf, p) {
                Ok(g) => g,
                Err(e) => {
                    println!("  p{:<2} geometry error: {e}", p + 1);
                    continue;
                }
            };
            let nchars = g.chars.len();
            let nlines = g.lines.len();
            if nchars > 0 {
                text_pages += 1;
            }
            // Mirror smart_pdf: feed embedded-image bounds to the lattice so
            // in-cell images show up as [IMAGE:img<N>] markers.
            let placed: Vec<table_lattice::PlacedImage> = engine
                .embedded_images(&pdf, p, 16, false)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|img| {
                    img.bounds
                        .map(|(x0, y0, x1, y1)| table_lattice::PlacedImage {
                            label: format!("img{}", img.index),
                            x0,
                            y0,
                            x1,
                            y1,
                        })
                })
                .collect();
            let lat = table_lattice::reconstruct(&g.chars, &g.lines, &placed);
            let stream = if lat.is_none() {
                table_stream::reconstruct(&g.chars)
            } else {
                None
            };
            match lat.as_ref().or(stream.as_ref()) {
                Some(t) => {
                    table_pages += 1;
                    let kind = if lat.is_some() { "lattice" } else { "stream " };
                    let merged = t.html.contains("colspan") || t.html.contains("rowspan");
                    if merged {
                        merged_pages += 1;
                    }
                    // Mirror the smart_pdf acceptance gate: cells>=4 && cov>=0.5
                    // && (conf>=0.3 || cov>=0.7).
                    let kept = t.n_rows * t.n_cols >= 4
                        && t.char_coverage >= 0.5
                        && (t.confidence >= 0.3 || t.char_coverage >= 0.7);
                    println!(
                        "  p{:<2} chars={:<5} lines={:<4} -> {} {}x{} conf={:.2} cov={:.2} merged={} [{}]",
                        p + 1,
                        nchars,
                        nlines,
                        kind,
                        t.n_rows,
                        t.n_cols,
                        t.confidence,
                        t.char_coverage,
                        merged,
                        if kept { "KEPT" } else { "drop" }
                    );
                    if std::env::var("DUMP").is_ok() {
                        let preview: String = t
                            .linearized
                            .replace('\n', " / ")
                            .chars()
                            .take(400)
                            .collect();
                        println!("       linz: {preview}");
                    }
                }
                None => {
                    println!(
                        "  p{:<2} chars={:<5} lines={:<4} -> NO TABLE",
                        p + 1,
                        nchars,
                        nlines
                    );
                }
            }
        }
        let layer = if text_pages == 0 {
            "SCANNED / image-only (NO text layer) -> needs OCR/ML"
        } else {
            "DIGITAL (text layer present)"
        };
        println!(
            "  SUMMARY: {layer}; text_pages={text_pages}/{} table_pages={table_pages} merged_pages={merged_pages}",
            pages.min(MAX_PAGES)
        );
    }
}
