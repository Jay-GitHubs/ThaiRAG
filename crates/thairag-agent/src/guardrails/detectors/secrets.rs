use std::sync::OnceLock;

use regex::Regex;

use crate::guardrails::types::{GuardStage, Violation, ViolationCode};

fn aws_key_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b").unwrap())
}

fn jwt_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\b").unwrap()
    })
}

fn github_pat_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // GitHub PATs: ghp_ / gho_ / ghu_ / ghs_ / ghr_ + 36 alnum chars.
    R.get_or_init(|| Regex::new(r"\bgh[psoru]_[A-Za-z0-9]{36,}\b").unwrap())
}

fn generic_api_key_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Conservative: long opaque tokens after key= / token= / Bearer.
    R.get_or_init(|| {
        Regex::new(r"(?i)\b(?:api[_-]?key|secret[_-]?key|access[_-]?token|bearer)[=:\s]+[A-Za-z0-9_\-]{24,}\b").unwrap()
    })
}

pub fn detect_secrets(text: &str, stage: GuardStage) -> Vec<Violation> {
    let mut out = Vec::new();
    push_matches(
        text,
        aws_key_re(),
        ViolationCode::SecretAwsKey,
        stage,
        &mut out,
    );
    push_matches(text, jwt_re(), ViolationCode::SecretJwt, stage, &mut out);
    push_matches(
        text,
        github_pat_re(),
        ViolationCode::SecretGithubPat,
        stage,
        &mut out,
    );
    push_matches(
        text,
        generic_api_key_re(),
        ViolationCode::SecretGenericApiKey,
        stage,
        &mut out,
    );
    out
}

fn push_matches(
    text: &str,
    re: &Regex,
    code: ViolationCode,
    stage: GuardStage,
    out: &mut Vec<Violation>,
) {
    for m in re.find_iter(text) {
        out.push(Violation {
            code,
            severity: code.default_severity(),
            stage,
            matched: m.as_str().to_string(),
            range: (m.start(), m.end()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_access_key() {
        let v = detect_secrets("AWS_KEY=AKIAIOSFODNN7EXAMPLE here", GuardStage::Output);
        assert!(v.iter().any(|x| x.code == ViolationCode::SecretAwsKey));
    }

    #[test]
    fn detects_jwt() {
        // Synthetic JWT-shaped string (header.payload.signature).
        let token =
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let v = detect_secrets(&format!("token: {token}"), GuardStage::Output);
        assert!(v.iter().any(|x| x.code == ViolationCode::SecretJwt));
    }

    #[test]
    fn detects_github_pat() {
        let v = detect_secrets(
            "auth: ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            GuardStage::Output,
        );
        assert!(v.iter().any(|x| x.code == ViolationCode::SecretGithubPat));
    }

    #[test]
    fn ignores_short_random_tokens() {
        let v = detect_secrets("hello world key=short", GuardStage::Output);
        assert!(v.is_empty());
    }
}
