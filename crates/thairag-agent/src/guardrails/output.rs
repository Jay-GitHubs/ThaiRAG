use thairag_config::schema::GuardrailsConfig;
use tracing::{debug, warn};

use crate::guardrails::detectors::{detect_blocklist, pii, secrets};
use crate::guardrails::input::redact;
use crate::guardrails::types::{GuardAction, GuardStage, GuardVerdict, Violation};

/// Truncates `text` to at most `max_chars` characters on a UTF-8 boundary.
/// Returns the (possibly shorter) slice plus a flag indicating whether
/// truncation happened.
fn bounded_scan_slice(text: &str, max_chars: usize) -> (&str, bool) {
    let mut iter = text.char_indices();
    if let Some((idx, _)) = iter.nth(max_chars) {
        (&text[..idx], true)
    } else {
        (text, false)
    }
}

/// Output-side guardrails. Runs after response generation.
///
/// Behavior:
/// - PII / secret leaks → action follows `policy.output_on_violation`
///   ("block" | "redact" | "regenerate"). Default: redact (least disruptive).
/// - Detector errors → fail-open (don't break the response).
/// - Prompt-injection patterns are not run on outputs (would false-positive on
///   legitimate text describing security topics).
pub struct OutputGuardrails {
    policy: GuardrailsConfig,
}

impl OutputGuardrails {
    pub fn new(policy: GuardrailsConfig) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &GuardrailsConfig {
        &self.policy
    }

    /// Redact `text` using the supplied violations and this guard's configured
    /// redaction token. Exposed so callers that receive a `Regenerate` action
    /// but have no retry pathway can still safely return redacted output
    /// instead of leaking the unfiltered response.
    pub fn sanitize(&self, text: &str, violations: &[Violation]) -> String {
        redact(text, violations, &self.policy.redaction_token)
    }

    pub fn check(&self, response: &str) -> GuardVerdict {
        let stage = GuardStage::Output;
        let mut violations: Vec<Violation> = Vec::new();

        // Bound the input to detectors so a runaway response can't pin a CPU
        // running every regex over megabytes of text. Redaction still applies
        // to the full response — only the *scan* is truncated.
        let (scan, truncated) = bounded_scan_slice(response, self.policy.max_response_chars);
        if truncated {
            debug!(
                limit = self.policy.max_response_chars,
                "Output guardrails: response exceeded scan budget; truncating detector input"
            );
        }

        if self.policy.detect_thai_id {
            violations.extend(pii::detect_thai_id(scan, stage));
        }
        if self.policy.detect_thai_phone {
            violations.extend(pii::detect_thai_phone(scan, stage));
        }
        if self.policy.detect_email {
            violations.extend(pii::detect_email(scan, stage));
        }
        if self.policy.detect_credit_card {
            violations.extend(pii::detect_credit_card(scan, stage));
        }
        if self.policy.detect_secrets {
            violations.extend(secrets::detect_secrets(scan, stage));
        }
        violations.extend(detect_blocklist(
            scan,
            &self.policy.blocklist_phrases,
            stage,
        ));

        if violations.is_empty() {
            return GuardVerdict::pass();
        }

        let codes: Vec<&str> = violations.iter().map(|v| v.code.as_str()).collect();
        debug!(?codes, "Output guardrails flagged violations");

        let action = match self.policy.output_on_violation.as_str() {
            "block" => GuardAction::Block {
                reason: "Response withheld due to policy violations.".into(),
            },
            "regenerate" => GuardAction::Regenerate {
                feedback: format!(
                    "Your previous response contained content blocked by policy ({}). \
                     Rewrite without including any personal data, secrets, or blocked phrases.",
                    codes.join(", ")
                ),
            },
            // Default: redact in place.
            _ => GuardAction::Sanitize(redact(response, &violations, &self.policy.redaction_token)),
        };

        if matches!(action, GuardAction::Block { .. }) {
            warn!(?codes, "Output guardrails: BLOCK");
        }

        GuardVerdict { action, violations }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(builder: impl FnOnce(&mut GuardrailsConfig)) -> GuardrailsConfig {
        let mut c = GuardrailsConfig::default();
        builder(&mut c);
        c
    }

    #[test]
    fn redacts_email_in_output() {
        let g = OutputGuardrails::new(config(|c| {
            c.detect_email = true;
            c.output_on_violation = "redact".into();
        }));
        let v = g.check("Contact: jay@example.com for help.");
        match v.action {
            GuardAction::Sanitize(s) => {
                assert!(s.contains("[REDACTED]"));
                assert!(!s.contains("jay@example.com"));
            }
            other => panic!("expected sanitize, got {other:?}"),
        }
    }

    #[test]
    fn blocks_when_policy_is_block() {
        let g = OutputGuardrails::new(config(|c| {
            c.detect_secrets = true;
            c.output_on_violation = "block".into();
        }));
        let v = g.check("Here is your key: AKIAIOSFODNN7EXAMPLE");
        assert!(matches!(v.action, GuardAction::Block { .. }));
    }

    #[test]
    fn regenerates_when_policy_is_regenerate() {
        let g = OutputGuardrails::new(config(|c| {
            c.detect_email = true;
            c.output_on_violation = "regenerate".into();
        }));
        let v = g.check("ping jay@example.com");
        assert!(matches!(v.action, GuardAction::Regenerate { .. }));
    }

    #[test]
    fn sanitize_redacts_using_policy_token() {
        // Exposed so the pipeline can fall back to redaction when a Regenerate
        // verdict comes back but no retry pathway is available.
        let g = OutputGuardrails::new(config(|c| {
            c.detect_email = true;
            c.output_on_violation = "regenerate".into();
            c.redaction_token = "[hidden]".into();
        }));
        let verdict = g.check("Contact: jay@example.com");
        let cleaned = g.sanitize("Contact: jay@example.com", &verdict.violations);
        assert!(!cleaned.contains("jay@example.com"));
        assert!(cleaned.contains("[hidden]"));
    }

    #[test]
    fn caps_detector_scan_at_max_response_chars() {
        // Detectors should not run over arbitrarily large output. With the cap
        // set just below the email's position, the email past the cutoff must
        // *not* be flagged.
        let prefix: String = "x".repeat(100);
        let body = format!("{prefix} leak: jay@example.com");
        let cutoff = prefix.len() + 5; // before "leak:"
        let g = OutputGuardrails::new(config(|c| {
            c.detect_email = true;
            c.max_response_chars = cutoff;
            c.output_on_violation = "redact".into();
        }));
        let v = g.check(&body);
        assert!(v.passed(), "scan should not see the truncated email");
    }
}
