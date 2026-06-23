//! Document-complexity profiler (Phase 1 of the complexity-routing roadmap —
//! see `docs/DOCUMENT_COMPLEXITY_ROUTING_DESIGN.md`).
//!
//! Classifies every region of every input document into a complexity class and
//! the fidelity tier it would route to, then prints a per-document report and a
//! corpus-wide distribution. It reuses the pipeline's REAL signal functions
//! (`extract_pages` / `select_page_strategy` / `text_layer_garbled`), so the
//! profile reflects actual routing decisions, not a reimplementation. No vision,
//! no AI, no DB — purely descriptive, so we can see a corpus's complexity
//! distribution and where extraction needs OCR/VLM today.
//!
//! Run:
//!   cargo run -p thairag-document --example profile_corpus -- <file-or-dir>...
//!   cargo run -p thairag-document --example profile_corpus -- --json <path>...

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use thairag_document::pdfium_engine;
use thairag_document::smart_pdf::{SmartPdfConfig, extract_pages};
use thairag_document::text_utils::text_layer_garbled;

/// Complexity class for one region (a PDF page, or a whole non-PDF document).
/// Generalizes `PageStrategy` with the corruption case split out.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    NativeText,    // clean text layer → tier 1
    NativeTable,   // deterministic table reconstruction → tier 1
    TabularAsText, // table-shaped, not reconstructable; raw text kept → tier 1
    Mixed,         // text + embedded figures → tier 1 text + tier 3 figures
    ImageHeavy,    // text + dominant imagery → tier 2/3
    Scanned,       // no usable text layer → tier 2/3
    CorruptedText, // text present but garbled CMap → tier 2/3
    NativeStruct,  // DOCX/XLSX/HTML structured (document-level) → tier 1
    DirectImage,   // image upload → tier 3
    Unsupported,
}

impl Class {
    fn label(self) -> &'static str {
        match self {
            Class::NativeText => "NativeText",
            Class::NativeTable => "NativeTable",
            Class::TabularAsText => "TabularAsText",
            Class::Mixed => "Mixed",
            Class::ImageHeavy => "ImageHeavy",
            Class::Scanned => "Scanned",
            Class::CorruptedText => "CorruptedText",
            Class::NativeStruct => "NativeStruct",
            Class::DirectImage => "DirectImage",
            Class::Unsupported => "Unsupported",
        }
    }

    /// Highest fidelity tier this class can be served at: 1 = native/exact,
    /// 2 = deterministic OCR, 3 = vision LLM.
    fn tier(self) -> u8 {
        match self {
            Class::NativeText | Class::NativeTable | Class::TabularAsText | Class::NativeStruct => {
                1
            }
            Class::Mixed => 1, // text exact; figures need tier 3 (counted separately)
            Class::ImageHeavy | Class::Scanned | Class::CorruptedText => 2,
            Class::DirectImage => 3,
            Class::Unsupported => 0,
        }
    }
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "html" | "htm" => "text/html",
        "png" | "jpg" | "jpeg" | "webp" | "gif" => "image",
        "txt" | "md" | "csv" => "text",
        _ => "unknown",
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

fn classify_pdf_page(ex: &thairag_document::smart_pdf::PageExtract) -> Class {
    use thairag_document::semantic::PageStrategy;
    if ex.stitched_into.is_some() {
        return Class::NativeTable; // content lives on the anchor page
    }
    if ex.table.is_some() {
        return Class::NativeTable;
    }
    match ex.strategy {
        PageStrategy::TextOnly => Class::NativeText,
        PageStrategy::Tabular => Class::TabularAsText,
        PageStrategy::Mixed => Class::Mixed,
        PageStrategy::ImageHeavy => Class::ImageHeavy,
        // A garbled text layer was upgraded to Scanned by extract_pages; the raw
        // text still carries the corruption, so distinguish it from a true scan.
        PageStrategy::Scanned => {
            if text_layer_garbled(&ex.text) {
                Class::CorruptedText
            } else {
                Class::Scanned
            }
        }
    }
}

fn profile_file(path: &Path, agg: &mut Agg, json_rows: &mut Vec<String>) {
    let mime = mime_for(path);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    agg.docs += 1;

    if mime == "application/pdf" {
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
            *per.entry(c.label()).or_default() += 1;
            *agg.class_counts.entry(c.label()).or_default() += 1;
            agg.total_regions += 1;
            if c == Class::Mixed && !ex.embedded.is_empty() {
                agg.figure_regions += 1;
            }
            if c.tier() == 1 {
                agg.tier1 += 1;
            } else if c.tier() >= 2 {
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
    let class = match mime {
        m if m.starts_with("application/vnd.openxml") || m == "text/html" => Class::NativeStruct,
        "image" => Class::DirectImage,
        "text" => Class::NativeText,
        _ => Class::Unsupported,
    };
    *agg.class_counts.entry(class.label()).or_default() += 1;
    agg.total_regions += 1;
    match class.tier() {
        1 => agg.tier1 += 1,
        2 | 3 => {
            agg.needs_model += 1;
            agg.docs_needing_ocr += 1;
        }
        _ => {}
    }
    println!("  {name}  [{}]  {}", mime, class.label());
    json_rows.push(format!(
        "{{\"file\":{name:?},\"class\":{:?}}}",
        class.label()
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
