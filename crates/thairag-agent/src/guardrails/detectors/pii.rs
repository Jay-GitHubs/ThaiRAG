use std::sync::OnceLock;

use regex::Regex;

use crate::guardrails::types::{GuardStage, Violation, ViolationCode};

fn thai_id_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b\d{13}\b").unwrap())
}

fn thai_phone_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Matches:
    //   +66 XX XXX XXXX  / +66 X XXXX XXXX  / +66XXXXXXXXX
    //   02-XXX-XXXX (9-digit landline)  /  0XX-XXX-XXXX (10-digit mobile)
    R.get_or_init(|| {
        Regex::new(
            r"(?:\+66[\s\-]?\d{1,2}[\s\-]?\d{3,4}[\s\-]?\d{4})|(?:\b0\d{1,2}[\s\-]?\d{3,4}[\s\-]?\d{4}\b)",
        )
        .unwrap()
    })
}

fn email_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap())
}

fn cc_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // 13–19 digits, optionally separated by spaces or dashes.
    R.get_or_init(|| Regex::new(r"\b(?:\d[ \-]?){13,19}\b").unwrap())
}

/// Verify a Thai national ID using the official mod-11 checksum.
/// digits[0..12] * (13..=2), sum % 11; check digit = (11 - sum%11) % 10.
pub fn is_valid_thai_id(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() != 13 {
        return false;
    }
    let sum: u32 = digits[..12]
        .iter()
        .enumerate()
        .map(|(i, d)| d * (13 - i as u32))
        .sum();
    let check = (11 - (sum % 11)) % 10;
    check == digits[12]
}

/// Luhn validator for credit-card numbers (strips non-digits first).
pub fn is_valid_luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let parity = digits.len() % 2;
    for (i, &d) in digits.iter().enumerate() {
        let v = if i % 2 == parity {
            let dd = d * 2;
            if dd > 9 { dd - 9 } else { dd }
        } else {
            d
        };
        sum += v;
    }
    sum.is_multiple_of(10)
}

pub fn detect_thai_id(text: &str, stage: GuardStage) -> Vec<Violation> {
    let mut out = Vec::new();
    for m in thai_id_re().find_iter(text) {
        if is_valid_thai_id(m.as_str()) {
            out.push(Violation {
                code: ViolationCode::PiiThaiId,
                severity: ViolationCode::PiiThaiId.default_severity(),
                stage,
                matched: m.as_str().to_string(),
                range: (m.start(), m.end()),
            });
        }
    }
    out
}

pub fn detect_thai_phone(text: &str, stage: GuardStage) -> Vec<Violation> {
    thai_phone_re()
        .find_iter(text)
        .map(|m| Violation {
            code: ViolationCode::PiiThaiPhone,
            severity: ViolationCode::PiiThaiPhone.default_severity(),
            stage,
            matched: m.as_str().to_string(),
            range: (m.start(), m.end()),
        })
        .collect()
}

pub fn detect_email(text: &str, stage: GuardStage) -> Vec<Violation> {
    email_re()
        .find_iter(text)
        .map(|m| Violation {
            code: ViolationCode::PiiEmail,
            severity: ViolationCode::PiiEmail.default_severity(),
            stage,
            matched: m.as_str().to_string(),
            range: (m.start(), m.end()),
        })
        .collect()
}

pub fn detect_credit_card(text: &str, stage: GuardStage) -> Vec<Violation> {
    let mut out = Vec::new();
    for m in cc_re().find_iter(text) {
        if is_valid_luhn(m.as_str()) {
            out.push(Violation {
                code: ViolationCode::PiiCreditCard,
                severity: ViolationCode::PiiCreditCard.default_severity(),
                stage,
                matched: m.as_str().to_string(),
                range: (m.start(), m.end()),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thai_id_checksum_valid() {
        // Known valid test ID (mod-11 checksum).
        assert!(is_valid_thai_id("1101700230708"));
    }

    #[test]
    fn thai_id_checksum_invalid() {
        assert!(!is_valid_thai_id("1234567890123"));
        assert!(!is_valid_thai_id("0000000000000"));
        assert!(!is_valid_thai_id("123"));
    }

    #[test]
    fn detects_valid_thai_id_in_text() {
        let v = detect_thai_id("My ID is 1101700230708 thanks", GuardStage::Input);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, ViolationCode::PiiThaiId);
    }

    #[test]
    fn skips_invalid_13_digit_strings() {
        let v = detect_thai_id("Order 1234567890123 was shipped", GuardStage::Input);
        assert_eq!(v.len(), 0);
    }

    #[test]
    fn detects_thai_phone_formats() {
        for sample in &[
            "Call 02-123-4567",
            "Tel +66 81 234 5678",
            "ติดต่อ 081-234-5678",
        ] {
            let v = detect_thai_phone(sample, GuardStage::Input);
            assert!(!v.is_empty(), "expected match in: {sample}");
        }
    }

    #[test]
    fn detects_email() {
        let v = detect_email("Contact me at jay@example.com", GuardStage::Input);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn luhn_valid_card() {
        assert!(is_valid_luhn("4242 4242 4242 4242"));
        assert!(is_valid_luhn("4111111111111111"));
    }

    #[test]
    fn luhn_invalid_card() {
        assert!(!is_valid_luhn("4242 4242 4242 4241"));
    }

    #[test]
    fn detects_only_luhn_valid_credit_cards() {
        let v = detect_credit_card("Card 4242 4242 4242 4242 ok", GuardStage::Input);
        assert_eq!(v.len(), 1);
        let v2 = detect_credit_card("Order 1234567890123 was shipped", GuardStage::Input);
        assert_eq!(v2.len(), 0);
    }
}
