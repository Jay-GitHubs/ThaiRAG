//! Heuristics for detecting "meaningfully empty" extracted text.
//!
//! Raw character counts mislead: a PowerPoint-exported PDF page often
//! contains only a page number ("- 1 -", "Page 1 of 12") or stray
//! whitespace from rasterized slides. These should be treated as empty
//! and routed to the vision-OCR fallback.

/// Strip trivial scaffolding (page numbers, repeated whitespace, common
/// header/footer patterns) and count remaining characters.
///
/// Used to decide whether a page or document needs the vision fallback.
pub fn meaningful_char_count(text: &str) -> usize {
    strip_trivial(text).trim().chars().count()
}

/// Return a cleaned copy of `text` with trivial scaffolding removed.
/// Exposed for tests / diagnostic logging.
pub fn strip_trivial(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if is_trivial_line(line) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Above this ratio of "alien" Latin-Extended letters to Thai letters, a Thai
/// PDF's text layer is considered corrupted. Some Thai PDFs ship a broken
/// ToUnicode CMap that maps Thai glyphs to Latin Extended-A/B codepoints
/// (`Ļ Ŀ ļ`, U+0100–U+024F), so *every* text extractor — pdf-extract and pdfium
/// alike — reads garbage like `เรืĻอง` / `โซลูชันĻ`. The only fix is to ignore
/// the text layer and OCR the rendered page. Measured: clean Thai PDFs score
/// 0.000; a corrupted real-world corpus scored 0.013–0.032.
const GARBLE_RATIO_THRESHOLD: f64 = 0.01;

/// Require a few absolute hits as well, so one stray Latin-Extended letter in a
/// short snippet (e.g. a foreign proper noun) can't trip the detector.
const GARBLE_MIN_ALIEN: usize = 3;

/// Whether a Thai text layer looks corrupted by a broken ToUnicode CMap (Latin
/// Extended-A/B letters leaking into Thai). When true the page's extracted text
/// is untrustworthy and should be replaced by vision OCR of the rendered page,
/// not used verbatim. See [`GARBLE_RATIO_THRESHOLD`].
pub fn text_layer_garbled(text: &str) -> bool {
    let mut alien = 0usize;
    let mut thai = 0usize;
    for c in text.chars() {
        let u = c as u32;
        if (0x0E00..=0x0E7F).contains(&u) {
            thai += 1;
        } else if (0x0100..=0x024F).contains(&u) {
            alien += 1;
        }
    }
    thai > 0 && alien >= GARBLE_MIN_ALIEN && (alien as f64 / thai as f64) >= GARBLE_RATIO_THRESHOLD
}

/// Heuristics: a line is "trivial" if it conveys no real content.
/// Matches typical scaffolding produced by PDF/PowerPoint exports.
fn is_trivial_line(line: &str) -> bool {
    // Bare page numbers: "1", "12", "- 1 -", "—1—", "* 3 *"
    let stripped: String = line
        .chars()
        .filter(|c| !c.is_whitespace() && !is_page_decoration(*c))
        .collect();
    if stripped.is_empty() {
        return true;
    }
    if stripped.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    // "Page 1", "Page 1 of 12", "หน้า 1", "หน้า 1 / 12"
    let lower = line.to_lowercase();
    if lower.starts_with("page ")
        && lower
            .trim_start_matches("page ")
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    {
        return true;
    }
    if line.starts_with("หน้า ")
        && line
            .trim_start_matches("หน้า ")
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    {
        return true;
    }

    // Repeated separator runs: "------", "======", "______"
    if line.len() >= 3
        && line
            .chars()
            .all(|c| matches!(c, '-' | '=' | '_' | '*' | '·'))
    {
        return true;
    }

    false
}

fn is_page_decoration(c: char) -> bool {
    matches!(c, '-' | '_' | '*' | '·' | '—' | '–' | '/' | '|' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_counts_zero() {
        assert_eq!(meaningful_char_count(""), 0);
        assert_eq!(meaningful_char_count("   \n\n  \t  "), 0);
    }

    #[test]
    fn bare_page_numbers_stripped() {
        assert_eq!(meaningful_char_count("1"), 0);
        assert_eq!(meaningful_char_count("- 1 -"), 0);
        assert_eq!(meaningful_char_count("— 12 —"), 0);
        assert_eq!(meaningful_char_count("* 3 *"), 0);
    }

    #[test]
    fn page_n_of_n_stripped() {
        assert_eq!(meaningful_char_count("Page 1"), 0);
        assert_eq!(meaningful_char_count("Page 1 of 12"), 0);
        assert_eq!(meaningful_char_count("page 5"), 0);
        assert_eq!(meaningful_char_count("หน้า 1"), 0);
        assert_eq!(meaningful_char_count("หน้า 1 / 12"), 0);
    }

    #[test]
    fn separator_runs_stripped() {
        assert_eq!(meaningful_char_count("---"), 0);
        assert_eq!(meaningful_char_count("======"), 0);
        assert_eq!(meaningful_char_count("______"), 0);
        assert_eq!(meaningful_char_count("****"), 0);
    }

    #[test]
    fn real_content_preserved() {
        let text = "This is a real paragraph with words.";
        assert_eq!(meaningful_char_count(text), text.len());
    }

    #[test]
    fn mixed_content_counts_only_real_lines() {
        let text = "Page 1\n\nThis is real content.\n\n- 1 -";
        assert_eq!(
            meaningful_char_count(text),
            "This is real content.".chars().count()
        );
    }

    #[test]
    fn powerpoint_exported_pdf_page_detected_empty() {
        // Typical PowerPoint-PDF page that yields only a page number
        let pdf_extracted = "\n\n   \n  - 1 -  \n\n";
        assert_eq!(meaningful_char_count(pdf_extracted), 0);
    }

    #[test]
    fn thai_page_marker_with_slides_yields_zero() {
        let text = "  หน้า 3 / 24  \n\n\n";
        assert_eq!(meaningful_char_count(text), 0);
    }

    #[test]
    fn clean_thai_is_not_garbled() {
        // 084_2568's text layer: pure Thai, no alien letters → ratio 0.000.
        let clean = "ประกาศเริ่มใช้งานไมโครเพย์เวอร์ชั่นใหม่ วันที่มีผลบังคับใช้";
        assert!(!text_layer_garbled(clean));
    }

    #[test]
    fn corrupted_cmap_thai_is_garbled() {
        // Digital Fraud's text layer: Thai with Latin-Extended leakage
        // (Ļ U+013B, Ŀ U+013F, ļ U+013C) from a broken ToUnicode CMap.
        let garbled = "ลิขสิทธิĿ สำหรับ บริษัท ไทยไมโครเพย์ดิจิทัล โซลูชันĻ จำกัด เท่านัļน เรืĻอง";
        assert!(text_layer_garbled(garbled));
    }

    #[test]
    fn one_foreign_letter_does_not_trip_detector() {
        // A single Latin-Extended letter (e.g. a foreign name) in otherwise
        // clean Thai must not be flagged — below the absolute-count floor.
        let mostly_clean = "เอกสารภาษาไทยที่สมบูรณ์ มีชื่อ Łódź ปรากฏหนึ่งครั้ง และข้อความอื่นอีกมาก";
        assert!(!text_layer_garbled(mostly_clean));
    }

    #[test]
    fn pure_english_is_not_garbled() {
        // No Thai → never flagged (the detector is Thai-CMap specific).
        assert!(!text_layer_garbled(
            "Just plain English text, nothing to see."
        ));
    }
}
