//! Document-complexity profiler (Phase 1 of the complexity-routing roadmap —
//! see `docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`).
//!
//! Classifies every region of every input document into a complexity class and
//! the fidelity tier it would route to, then prints a per-document report and a
//! corpus-wide distribution. Classification goes through the unified
//! `region_router` (Phase 2), fed the pipeline's REAL signals (`extract_pages` /
//! `PageStrategy` / `text_layer_garbled`), so the profile reflects actual routing
//! decisions. No vision, no AI, no DB — purely descriptive, so we can see a
//! corpus's complexity distribution and where extraction needs OCR/VLM today.
//!
//! Run:
//!   cargo run -p thairag-document --example profile_corpus -- <file-or-dir>...
//!   cargo run -p thairag-document --example profile_corpus -- --json <path>...

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thairag_document::pdfium_engine;
use thairag_document::region_router::{
    FidelityTier, RegionClass, RegionSignals, SourceFormat, classify,
};
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};
use thairag_document::text_utils::text_layer_garbled;

fn format_for(path: &Path) -> SourceFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => SourceFormat::Pdf,
        "docx" => SourceFormat::Docx,
        "xlsx" => SourceFormat::Xlsx,
        "html" | "htm" => SourceFormat::Html,
        "png" | "jpg" | "jpeg" | "webp" | "gif" => SourceFormat::Image,
        "txt" | "md" | "csv" => SourceFormat::Text,
        _ => SourceFormat::Other,
    }
}

#[derive(Default)]
struct Agg {
    class_counts: BTreeMap<&'static str, usize>,
    tier1: usize,
    needs_model: usize, // tier 2/3 regions
    figure_regions: usize,
    total_regions: usize,
    docs: usize,
    docs_needing_ocr: usize,
}

/// Classify a PDF page via the unified router, from the already-selected
/// `PageStrategy` plus the table/garble refinement flags.
fn classify_pdf_page(ex: &thairag_document::smart_pdf::PageExtract) -> RegionClass {
    // A stitched continuation page's content lives on the anchor (a table).
    let has_table = ex.table.is_some() || ex.stitched_into.is_some();
    RegionClass::from_page_strategy(ex.strategy, text_layer_garbled(&ex.text), has_table)
}

fn fmt_label(f: SourceFormat) -> &'static str {
    match f {
        SourceFormat::Pdf => "pdf",
        SourceFormat::Docx => "docx",
        SourceFormat::Xlsx => "xlsx",
        SourceFormat::Html => "html",
        SourceFormat::Image => "image",
        SourceFormat::Text => "text",
        SourceFormat::Other => "other",
    }
}

fn profile_file(path: &Path, agg: &mut Agg, json_rows: &mut Vec<String>) {
    let fmt = format_for(path);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    agg.docs += 1;

    if fmt == SourceFormat::Pdf {
        if !pdfium_engine::is_available() {
            println!("  {name}  (PDF — libpdfium unavailable, skipped)");
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            println!("  {name}  (unreadable)");
            return;
        };
        let pages = match extract_pages(&bytes, &SmartPdfConfig::default()) {
            Ok(p) => p,
            Err(e) => {
                println!("  {name}  (extract failed: {e})");
                return;
            }
        };
        let mut per: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut doc_needs = false;
        for ex in &pages {
            let c = classify_pdf_page(ex);
            *per.entry(c.as_str()).or_default() += 1;
            *agg.class_counts.entry(c.as_str()).or_default() += 1;
            agg.total_regions += 1;
            if c.needs_figure_description() && !ex.embedded.is_empty() {
                agg.figure_regions += 1;
            }
            if c.tier() == FidelityTier::Native {
                agg.tier1 += 1;
            } else {
                agg.needs_model += 1;
                doc_needs = true;
            }
        }
        if doc_needs {
            agg.docs_needing_ocr += 1;
        }
        let dist: Vec<String> = per.iter().map(|(k, v)| format!("{k}={v}")).collect();
        println!("  {name}  [{} pages]  {}", pages.len(), dist.join(" "));
        json_rows.push(format!(
            "{{\"file\":{:?},\"pages\":{},\"classes\":{{{}}}}}",
            name,
            pages.len(),
            per.iter()
                .map(|(k, v)| format!("{k:?}:{v}"))
                .collect::<Vec<_>>()
                .join(",")
        ));
        return;
    }

    // Non-PDF: document-level class (region-level profiling is Phase 5).
    let class = classify(&RegionSignals::document(fmt));
    *agg.class_counts.entry(class.as_str()).or_default() += 1;
    agg.total_regions += 1;
    if class.tier() == FidelityTier::Native {
        agg.tier1 += 1;
    } else {
        agg.needs_model += 1;
        agg.docs_needing_ocr += 1;
    }
    println!("  {name}  [{}]  {}", fmt_label(fmt), class.as_str());
    json_rows.push(format!(
        "{{\"file\":{name:?},\"class\":{:?}}}",
        class.as_str()
    ));
}

fn collect_inputs(args: &[String]) -> Vec<PathBuf> {
    let supported = [
        "pdf", "docx", "xlsx", "html", "htm", "png", "jpg", "jpeg", "webp", "gif", "txt", "md",
        "csv",
    ];
    let mut out = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    let ep = e.path();
                    if ep
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| supported.contains(&x.to_ascii_lowercase().as_str()))
                        .unwrap_or(false)
                    {
                        out.push(ep);
                    }
                }
            }
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.first().map(|a| a == "--json").unwrap_or(false);
    if json {
        args.remove(0);
    }
    if args.is_empty() {
        eprintln!(
            "usage: profile_corpus [--json] <file-or-dir>...\n\
             Profiles document complexity per region and prints the corpus distribution."
        );
        return;
    }

    let inputs = collect_inputs(&args);
    let mut agg = Agg::default();
    let mut json_rows: Vec<String> = Vec::new();

    println!("=== Per-document profile ({} files) ===", inputs.len());
    for path in &inputs {
        profile_file(path, &mut agg, &mut json_rows);
    }

    println!("\n=== Corpus complexity distribution ===");
    println!("documents:            {}", agg.docs);
    println!("regions (pages/docs): {}", agg.total_regions);
    for (k, v) in &agg.class_counts {
        let pct = if agg.total_regions > 0 {
            100.0 * *v as f64 / agg.total_regions as f64
        } else {
            0.0
        };
        println!("  {k:<14} {v:>5}  ({pct:.1}%)");
    }
    let det_pct = if agg.total_regions > 0 {
        100.0 * agg.tier1 as f64 / agg.total_regions as f64
    } else {
        0.0
    };
    println!("\nfidelity routing:");
    println!(
        "  tier 1 (deterministic/native): {:>5}  ({:.1}%)",
        agg.tier1, det_pct
    );
    println!(
        "  tier 2/3 (OCR / vision LLM):   {:>5}  ({:.1}%)",
        agg.needs_model,
        100.0 - det_pct
    );
    println!("  figure regions (need VLM desc): {}", agg.figure_regions);
    println!(
        "  documents needing OCR/vision on >=1 region: {}/{}",
        agg.docs_needing_ocr, agg.docs
    );

    if json {
        println!("\n=== JSON ===");
        println!(
            "{{\"documents\":{},\"regions\":{},\"tier1\":{},\"needs_model\":{},\"figure_regions\":{},\"per_file\":[{}]}}",
            agg.docs,
            agg.total_regions,
            agg.tier1,
            agg.needs_model,
            agg.figure_regions,
            json_rows.join(",")
        );
    }
}
