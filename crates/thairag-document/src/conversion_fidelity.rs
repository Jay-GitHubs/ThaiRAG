//! Deterministic conversion-fidelity assessment.
//!
//! Compares the converted text (what feeds the vector DB) against an
//! *independent* extraction of the original document's text — no LLM, so the
//! check itself cannot hallucinate. The sharp signal is numbers: numeric tokens
//! present in the original but missing from the output were dropped; numeric
//! tokens in the output but absent from the original were fabricated. Character
//! coverage catches broader text loss (and works for Thai without word
//! segmentation).
//!
//! When the original has no usable text layer (e.g. a scanned PDF), there is no
//! ground truth to compare against, so the result is `unverifiable` rather than
//! a fabricated score.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use thairag_core::models::ConversionFidelity;

const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// Below this many meaningful chars, the original is treated as having no text
/// layer (scanned / image-only) → unverifiable.
const MIN_GROUNDTRUTH_CHARS: usize = 30;
const VERIFIED_NUMBER_RECALL: f32 = 0.98;
const VERIFIED_CHAR_COVERAGE: f32 = 0.90;

fn unverifiable() -> ConversionFidelity {
    ConversionFidelity {
        status: "unverifiable".to_string(),
        score: 0.0,
        numbers_total: 0,
        numbers_matched: 0,
        numbers_fabricated: 0,
        char_coverage: 0.0,
    }
}

/// Assess how faithfully `converted_text` preserves the `original` document's
/// extractable content. May call native extractors (pdfium etc.), so run inside
/// `spawn_blocking`.
pub fn assess(original: &[u8], mime: &str, converted_text: &str) -> ConversionFidelity {
    let Some(ground) = ground_truth_text(original, mime) else {
        return unverifiable();
    };
    if meaningful_chars(&ground) < MIN_GROUNDTRUTH_CHARS {
        return unverifiable();
    }
    compare(&ground, &strip_markup(converted_text))
}

/// Independent extraction of the original's text, per format. `None` for formats
/// with no extractable text layer (images), so callers report `unverifiable`.
fn ground_truth_text(raw: &[u8], mime: &str) -> Option<String> {
    match mime {
        "application/pdf" => pdf_text(raw),
        DOCX_MIME => docx_text(raw),
        XLSX_MIME => xlsx_text(raw),
        "text/plain" | "text/markdown" | "text/csv" | "application/json" => {
            String::from_utf8(raw.to_vec()).ok()
        }
        "text/html" => html_text(raw),
        _ => None,
    }
}

fn pdf_text(raw: &[u8]) -> Option<String> {
    crate::pdfium_engine::extract_text_by_pages(raw)
        .ok()
        .map(|pages| {
            pages
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
                .join("\n")
        })
}

fn docx_text(raw: &[u8]) -> Option<String> {
    let docx = docx_rs::read_docx(raw).ok()?;
    let mut out = String::new();
    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                for pc in &p.children {
                    if let docx_rs::ParagraphChild::Run(run) = pc {
                        for rc in &run.children {
                            if let docx_rs::RunChild::Text(t) = rc {
                                out.push_str(&t.text);
                            }
                        }
                    }
                }
                out.push('\n');
            }
            docx_rs::DocumentChild::Table(table) => {
                for row in &table.rows {
                    let docx_rs::TableChild::TableRow(tr) = row;
                    for cell in &tr.cells {
                        let docx_rs::TableRowChild::TableCell(tc) = cell;
                        for content in &tc.children {
                            if let docx_rs::TableCellContent::Paragraph(p) = content {
                                for pc in &p.children {
                                    if let docx_rs::ParagraphChild::Run(run) = pc {
                                        for rc in &run.children {
                                            if let docx_rs::RunChild::Text(t) = rc {
                                                out.push_str(&t.text);
                                                out.push(' ');
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    Some(out)
}

fn xlsx_text(raw: &[u8]) -> Option<String> {
    use calamine::{Reader, open_workbook_auto_from_rs};
    let mut wb = open_workbook_auto_from_rs(Cursor::new(raw)).ok()?;
    let mut out = String::new();
    let names: Vec<String> = wb.sheet_names().to_vec();
    for name in &names {
        if let Ok(range) = wb.worksheet_range(name) {
            for row in range.rows() {
                for cell in row {
                    out.push_str(&cell.to_string());
                    out.push(' ');
                }
                out.push('\n');
            }
        }
    }
    Some(out)
}

fn html_text(raw: &[u8]) -> Option<String> {
    let html = String::from_utf8(raw.to_vec()).ok()?;
    let doc = scraper::Html::parse_document(&html);
    Some(doc.root_element().text().collect::<Vec<_>>().join(" "))
}

/// Count non-whitespace characters.
fn meaningful_chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

/// Remove HTML tags and `[IMAGE:…]` markers so only readable text remains.
fn strip_markup(s: &str) -> String {
    // Pass 1: replace anything between '<' and '>' with a SPACE — not nothing —
    // so adjacent cells (`</td><td>`) don't fuse their content (which would,
    // e.g., merge two numbers into one bogus token).
    let mut no_tags = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                no_tags.push(' ');
            }
            _ if !in_tag => no_tags.push(c),
            _ => {}
        }
    }
    // Pass 2: drop "[IMAGE:...]" markers.
    let chars: Vec<char> = no_tags.chars().collect();
    let mut out = String::with_capacity(no_tags.len());
    let mut idx = 0;
    while idx < chars.len() {
        if chars[idx] == '[' && chars[idx..].iter().take(7).collect::<String>() == "[IMAGE:" {
            while idx < chars.len() && chars[idx] != ']' {
                idx += 1;
            }
            idx += 1; // skip the ']'
        } else {
            out.push(chars[idx]);
            idx += 1;
        }
    }
    out
}

fn thai_digit(c: char) -> Option<char> {
    if ('\u{0E50}'..='\u{0E59}').contains(&c) {
        Some((b'0' + (c as u32 - 0x0E50) as u8) as char)
    } else {
        None
    }
}

/// Distinct numeric tokens. Thai digits are normalised to ASCII; thousands
/// commas are removed; leading/trailing dots trimmed. "๑,๒๓๔" and "1,234" both
/// become "1234"; "12.5" stays "12.5".
fn extract_numbers(s: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut run = String::new();
    let flush = |run: &mut String, set: &mut HashSet<String>| {
        if !run.is_empty() {
            let cleaned: String = run.chars().filter(|&c| c != ',').collect();
            let cleaned = cleaned.trim_matches('.');
            if cleaned.chars().any(|c| c.is_ascii_digit()) {
                set.insert(cleaned.to_string());
            }
            run.clear();
        }
    };
    for ch in s.chars() {
        let n = thai_digit(ch).unwrap_or(ch);
        if n.is_ascii_digit() || n == ',' || n == '.' {
            run.push(n);
        } else {
            flush(&mut run, &mut set);
        }
    }
    flush(&mut run, &mut set);
    set
}

/// Fraction of the original's non-space characters present in the output
/// (multiset intersection / original size). Thai digits normalised so a
/// digit-script change doesn't read as loss.
fn char_coverage(orig: &str, conv: &str) -> f32 {
    let bag = |s: &str| {
        let mut m: HashMap<char, usize> = HashMap::new();
        for c in s.chars() {
            if !c.is_whitespace() {
                let n = thai_digit(c).unwrap_or(c);
                *m.entry(n).or_insert(0) += 1;
            }
        }
        m
    };
    let o = bag(orig);
    let c = bag(conv);
    let total: usize = o.values().sum();
    if total == 0 {
        return 1.0;
    }
    let covered: usize = o
        .iter()
        .map(|(ch, &cnt)| cnt.min(*c.get(ch).unwrap_or(&0)))
        .sum();
    covered as f32 / total as f32
}

/// Pure comparison of an original text against a converted text.
fn compare(orig: &str, conv: &str) -> ConversionFidelity {
    let o_nums = extract_numbers(orig);
    let c_nums = extract_numbers(conv);
    let numbers_total = o_nums.len();
    let numbers_matched = o_nums.iter().filter(|n| c_nums.contains(*n)).count();
    let numbers_fabricated = c_nums.iter().filter(|n| !o_nums.contains(*n)).count();
    let number_recall = if numbers_total == 0 {
        1.0
    } else {
        numbers_matched as f32 / numbers_total as f32
    };
    let cov = char_coverage(orig, conv);

    let mut score = 0.5 * number_recall + 0.5 * cov;
    if numbers_fabricated > 0 {
        score *= 0.7; // visible penalty for possible fabrication
    }
    let status = if number_recall >= VERIFIED_NUMBER_RECALL
        && numbers_fabricated == 0
        && cov >= VERIFIED_CHAR_COVERAGE
    {
        "verified"
    } else {
        "review"
    };

    ConversionFidelity {
        status: status.to_string(),
        score,
        numbers_total,
        numbers_matched,
        numbers_fabricated,
        char_coverage: cov,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faithful_conversion_is_verified() {
        let orig = "ลำดับ 1 ราคา ๑,๒๓๔ บาท หมวด อาวุธ";
        let conv =
            "<table><tr><td>ลำดับ 1</td><td>ราคา 1,234 บาท</td><td>หมวด อาวุธ</td></tr></table>";
        let r = compare(orig, &strip_markup(conv));
        assert_eq!(r.status, "verified", "{r:?}");
        assert_eq!(r.numbers_fabricated, 0);
        assert!(r.numbers_matched == r.numbers_total && r.numbers_total >= 2);
        assert!(r.char_coverage > 0.95);
    }

    #[test]
    fn dropped_number_flags_review() {
        let orig = "A 100 B 200 C 300";
        let conv = "A 100 B 200"; // 300 dropped
        let r = compare(orig, conv);
        assert_eq!(r.status, "review");
        assert_eq!(r.numbers_total, 3);
        assert_eq!(r.numbers_matched, 2);
    }

    #[test]
    fn fabricated_number_flags_review() {
        let orig = "total 500";
        let conv = "total 500 and also 999"; // 999 fabricated
        let r = compare(orig, conv);
        assert_eq!(r.status, "review");
        assert_eq!(r.numbers_fabricated, 1);
    }

    #[test]
    fn thai_digits_match_arabic() {
        let mut a = extract_numbers("๕,๖๗๘");
        let b = extract_numbers("5,678");
        assert_eq!(a, b);
        a.insert("x".into()); // sanity: sets are real
        assert!(a.contains("5678"));
    }

    #[test]
    fn adjacent_numeric_cells_do_not_fuse() {
        // Two cells each holding only a number, no internal space. Stripping
        // tags must not merge "1" and "1234" into "11234".
        let orig = "Item 1 ๑,๒๓๔";
        let conv = "<table><tr><td>Item 1</td><td>๑,๒๓๔</td></tr></table>";
        let r = compare(orig, &strip_markup(conv));
        assert_eq!(r.numbers_fabricated, 0, "{r:?}");
        assert_eq!(r.numbers_matched, r.numbers_total);
        assert_eq!(r.status, "verified", "{r:?}");
    }

    #[test]
    fn strip_markup_removes_tags_and_image_markers() {
        let s = "before <td colspan=\"2\">cell</td> [IMAGE:abc-123] after";
        let out = strip_markup(s);
        assert!(out.contains("cell"));
        assert!(out.contains("before"));
        assert!(out.contains("after"));
        assert!(!out.contains("colspan"));
        assert!(!out.contains("IMAGE"));
    }
}
