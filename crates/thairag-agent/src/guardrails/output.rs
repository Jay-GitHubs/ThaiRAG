use thairag_config::schema::GuardrailsConfig;
use tracing::{debug, warn};

use crate::guardrails::detectors::{detect_blocklist, pii, secrets};
use crate::guardrails::input::redact;
use crate::guardrails::types::{GuardAction, GuardStage, GuardVerdict, Violation};

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

    pub fn check(&self, response: &str) -> GuardVerdict {
        let stage = GuardStage::Output;
        let mut violations: Vec<Violation> = Vec::new();

        if self.policy.detect_thai_id {
            violations.extend(pii::detect_thai_id(response, stage));
        }
        if self.policy.detect_thai_phone {
            violations.extend(pii::detect_thai_phone(response, stage));
        }
        if self.policy.detect_email {
            violations.extend(pii::detect_email(response, stage));
        }
        if self.policy.detect_credit_card {
            violations.extend(pii::detect_credit_card(response, stage));
        }
        if self.policy.detect_secrets {
            violations.extend(secrets::detect_secrets(response, stage));
        }
        violations.extend(detect_blocklist(
            response,
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
}
