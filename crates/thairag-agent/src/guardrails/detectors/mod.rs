pub mod injection;
pub mod pii;
pub mod secrets;

use regex::Regex;

use crate::guardrails::types::{GuardStage, Violation, ViolationCode};

/// Case-insensitive substring match against a list of operator-supplied phrases.
///
/// Builds one combined `(?i)` regex per call so that byte offsets returned by
/// `find_iter` always reference the original (un-cased) text. The previous
/// implementation lower-cased the text and reused those offsets — that breaks
/// for any codepoint whose lowercase form has a different byte length (e.g.
/// German ß → ss), silently corrupting redaction.
pub fn detect_blocklist(text: &str, phrases: &[String], stage: GuardStage) -> Vec<Violation> {
    let escaped: Vec<String> = phrases
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| regex::escape(p))
        .collect();
    if escaped.is_empty() {
        return Vec::new();
    }
    let pattern = format!("(?i)(?:{})", escaped.join("|"));
    let Ok(re) = Regex::new(&pattern) else {
        return Vec::new();
    };
    re.find_iter(text)
        .map(|m| Violation {
            code: ViolationCode::Blocklist,
            severity: ViolationCode::Blocklist.default_severity(),
            stage,
            matched: m.as_str().to_string(),
            range: (m.start(), m.end()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_case_insensitive_ascii() {
        let v = detect_blocklist(
            "Top SECRET project codename",
            &["secret".to_string()],
            GuardStage::Input,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].matched, "SECRET");
    }

    #[test]
    fn matches_thai_phrase() {
        let v = detect_blocklist(
            "นี่คือเอกสารลับมาก โปรดอย่าเผยแพร่",
            &["ลับมาก".to_string()],
            GuardStage::Input,
        );
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn empty_phrases_returns_empty() {
        let v = detect_blocklist("anything", &[], GuardStage::Input);
        assert!(v.is_empty());
    }

    #[test]
    fn ranges_index_original_text_for_non_ascii_lowercase() {
        // German ß lowercases to itself but uppercase ẞ → ß lengthens differently
        // in some Unicode versions. Using SS → ss as a clearer test: the byte
        // length differs across cases of the phrase, but matches must still
        // point at original byte offsets.
        let text = "Vorsicht: STRAßE ist gesperrt";
        let v = detect_blocklist(text, &["straße".to_string()], GuardStage::Input);
        assert_eq!(v.len(), 1);
        let (s, e) = v[0].range;
        // The slice from the original text at the reported range must match
        // the recorded `matched` string exactly.
        assert_eq!(&text[s..e], v[0].matched);
    }
}
