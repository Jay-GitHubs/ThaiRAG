use serde::{Deserialize, Serialize};

/// Closed enum of violation codes. Used as a Prometheus label, so additions
/// must be deliberate to avoid cardinality explosion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ViolationCode {
    QueryTooLong,
    PiiThaiId,
    PiiThaiPhone,
    PiiEmail,
    PiiCreditCard,
    SecretAwsKey,
    SecretJwt,
    SecretGithubPat,
    SecretGenericApiKey,
    PromptInjection,
    Blocklist,
}

impl ViolationCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::QueryTooLong => "QUERY_TOO_LONG",
            Self::PiiThaiId => "PII_THAI_ID",
            Self::PiiThaiPhone => "PII_THAI_PHONE",
            Self::PiiEmail => "PII_EMAIL",
            Self::PiiCreditCard => "PII_CREDIT_CARD",
            Self::SecretAwsKey => "SECRET_AWS_KEY",
            Self::SecretJwt => "SECRET_JWT",
            Self::SecretGithubPat => "SECRET_GITHUB_PAT",
            Self::SecretGenericApiKey => "SECRET_GENERIC_API_KEY",
            Self::PromptInjection => "PROMPT_INJECTION",
            Self::Blocklist => "BLOCKLIST",
        }
    }

    pub fn default_severity(&self) -> Severity {
        match self {
            Self::PiiThaiId | Self::PiiCreditCard | Self::SecretAwsKey => Severity::Critical,
            Self::SecretJwt | Self::SecretGithubPat | Self::SecretGenericApiKey => Severity::High,
            Self::PromptInjection => Severity::High,
            Self::PiiEmail | Self::PiiThaiPhone => Severity::Medium,
            Self::QueryTooLong | Self::Blocklist => Severity::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardStage {
    Input,
    Output,
}

impl GuardStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

/// A single match from a detector. The matched substring is kept here for
/// sanitization but **must not** be logged or persisted to the inference store.
#[derive(Debug, Clone)]
pub struct Violation {
    pub code: ViolationCode,
    pub severity: Severity,
    pub stage: GuardStage,
    /// The actual matched text. Kept for test inspection and future use by
    /// detector-specific redactors that need the raw match (e.g. partial
    /// masking like `****1234`). `pub(crate)` and `#[allow(dead_code)]` because
    /// no runtime code reads it today — redaction uses `range` only — but we
    /// don't want callers outside this crate touching matched PII.
    #[allow(dead_code)]
    pub(crate) matched: String,
    /// Byte range of the match within the scanned text.
    pub range: (usize, usize),
}

/// What the guardrails engine decided to do with a piece of text.
#[derive(Debug, Clone)]
pub enum GuardAction {
    /// Text passed all checks, no changes.
    Pass,
    /// Text was modified (e.g. PII redacted). Use the new value.
    Sanitize(String),
    /// Text is unsafe; refuse the request with `reason`.
    Block { reason: String },
    /// Output failed grounding/policy; ask the generator to retry with feedback.
    Regenerate { feedback: String },
}

/// Aggregate verdict from running a stage's detectors.
#[derive(Debug, Clone)]
pub struct GuardVerdict {
    pub action: GuardAction,
    pub violations: Vec<Violation>,
}

impl GuardVerdict {
    pub fn pass() -> Self {
        Self {
            action: GuardAction::Pass,
            violations: Vec::new(),
        }
    }

    pub fn passed(&self) -> bool {
        matches!(self.action, GuardAction::Pass)
    }
}
