pub mod injection;
pub mod pii;
pub mod secrets;

use crate::guardrails::types::{GuardStage, Violation, ViolationCode};

/// Case-insensitive substring match against a list of operator-supplied phrases.
pub fn detect_blocklist(text: &str, phrases: &[String], stage: GuardStage) -> Vec<Violation> {
    if phrases.is_empty() {
        return Vec::new();
    }
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for phrase in phrases {
        let needle = phrase.to_lowercase();
        if needle.is_empty() {
            continue;
        }
        let mut start = 0usize;
        while let Some(pos) = lower[start..].find(&needle) {
            let abs = start + pos;
            let end = abs + needle.len();
            out.push(Violation {
                code: ViolationCode::Blocklist,
                severity: ViolationCode::Blocklist.default_severity(),
                stage,
                matched: text.get(abs..end).unwrap_or("").to_string(),
                range: (abs, end),
            });
            start = end;
        }
    }
    out
}
