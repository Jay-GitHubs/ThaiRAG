use std::collections::BTreeMap;

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use thairag_agent::guardrails::{InputGuardrails, OutputGuardrails};
use thairag_auth::AuthClaims;
use thairag_config::schema::GuardrailsConfig;

use crate::routes::AppState;
use crate::routes::settings::InferenceLogFilterQuery;
use crate::store::InferenceLogEntry;

/// Aggregate stats for the guardrails monitoring dashboard.
#[derive(Debug, Serialize, Default)]
pub struct GuardrailsStats {
    /// Number of inference rows where input guardrails ran (pass + fail).
    pub input_checks_total: u64,
    /// Number of inference rows where output guardrails ran.
    pub output_checks_total: u64,
    /// Number of rows that fired at least one violation.
    pub violations_total: u64,
    /// Number of rows where input was blocked (pass=false).
    pub input_blocks_total: u64,
    /// Number of rows where output was blocked (pass=false).
    pub output_blocks_total: u64,
    /// Top violation codes (code → count), sorted descending by count.
    pub by_code: Vec<CodeCount>,
    /// Time-series buckets — count per code per bucket. Empty when no `bucket` requested.
    pub buckets: Vec<TimeBucket>,
}

#[derive(Debug, Serialize)]
pub struct CodeCount {
    pub code: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct TimeBucket {
    /// ISO date or hour bucket key (e.g. "2026-05-08T13").
    pub bucket: String,
    pub violations: u64,
    pub blocks: u64,
}

/// Per-violation summary for the violation log.
#[derive(Debug, Serialize)]
pub struct ViolationRow {
    pub timestamp: String,
    pub response_id: String,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub query_preview: String,
    pub codes: Vec<String>,
    pub input_pass: Option<bool>,
    pub output_pass: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ViolationsResponse {
    pub entries: Vec<ViolationRow>,
    pub total: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct PreviewRequest {
    /// Sample user query to test input guardrails against.
    pub query: Option<String>,
    /// Sample model response to test output guardrails against.
    pub response: Option<String>,
    /// Optional override; if absent, uses the global effective policy.
    pub policy: Option<GuardrailsConfig>,
}

#[derive(Debug, Serialize)]
pub struct PreviewVerdict {
    /// "pass" | "sanitize" | "block" | "regenerate"
    pub action: String,
    /// Codes only — never matched substrings.
    pub codes: Vec<String>,
    /// Sanitized text when action=sanitize, refusal reason when action=block, else null.
    pub output: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub input: Option<PreviewVerdict>,
    pub output: Option<PreviewVerdict>,
}

/// GET /api/km/guardrails/stats
/// Aggregate guardrail metrics over the supplied filter window.
pub async fn get_stats(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<InferenceLogFilterQuery>,
) -> Json<GuardrailsStats> {
    // Pull up to 50k rows in the window — same cap as export.
    let filter = q.to_filter(50_000);
    let entries = state.km_store.list_inference_logs(&filter);
    Json(aggregate_stats(&entries))
}

fn aggregate_stats(entries: &[InferenceLogEntry]) -> GuardrailsStats {
    let mut s = GuardrailsStats::default();
    let mut by_code: BTreeMap<String, u64> = BTreeMap::new();

    for e in entries {
        if e.input_guardrails_pass.is_some() {
            s.input_checks_total += 1;
            if e.input_guardrails_pass == Some(false) {
                s.input_blocks_total += 1;
            }
        }
        if e.output_guardrails_pass.is_some() {
            s.output_checks_total += 1;
            if e.output_guardrails_pass == Some(false) {
                s.output_blocks_total += 1;
            }
        }
        let codes = parse_codes(&e.guardrail_violation_codes);
        if !codes.is_empty() {
            s.violations_total += 1;
            for c in codes {
                *by_code.entry(c).or_insert(0) += 1;
            }
        }
    }

    let mut counts: Vec<CodeCount> = by_code
        .into_iter()
        .map(|(code, count)| CodeCount { code, count })
        .collect();
    counts.sort_by_key(|c| std::cmp::Reverse(c.count));
    s.by_code = counts;
    s
}

fn parse_codes(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// GET /api/km/guardrails/violations
/// Filtered list of inference logs that triggered at least one violation.
pub async fn list_violations(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<InferenceLogFilterQuery>,
) -> Json<ViolationsResponse> {
    let filter = q.to_filter(200);
    // We over-fetch and post-filter: most rows have no violations.
    let raw = state.km_store.list_inference_logs(&filter);
    let entries: Vec<ViolationRow> = raw
        .iter()
        .filter(|e| !e.guardrail_violation_codes.is_empty())
        .map(|e| ViolationRow {
            timestamp: e.timestamp.clone(),
            response_id: e.response_id.clone(),
            user_id: e.user_id.clone(),
            workspace_id: e.workspace_id.clone(),
            query_preview: e.query_text.chars().take(200).collect(),
            codes: parse_codes(&e.guardrail_violation_codes),
            input_pass: e.input_guardrails_pass,
            output_pass: e.output_guardrails_pass,
        })
        .collect();
    let total = entries.len() as u64;
    Json(ViolationsResponse { entries, total })
}

/// POST /api/km/guardrails/preview
/// Run input/output guardrails on supplied text without recording anything.
/// If `policy` is omitted, falls back to the global effective policy.
pub async fn preview(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Json(req): Json<PreviewRequest>,
) -> Json<PreviewResponse> {
    let policy = req.policy.unwrap_or_else(|| {
        crate::routes::settings::get_effective_chat_pipeline(&state)
            .guardrails
            .clone()
    });

    let input = req.query.as_deref().map(|q| {
        let g = InputGuardrails::new(policy.clone());
        verdict_to_preview(g.check(q))
    });
    let output = req.response.as_deref().map(|r| {
        let g = OutputGuardrails::new(policy.clone());
        verdict_to_preview(g.check(r))
    });

    Json(PreviewResponse { input, output })
}

fn verdict_to_preview(v: thairag_agent::guardrails::GuardVerdict) -> PreviewVerdict {
    let codes: Vec<String> = v
        .violations
        .iter()
        .map(|x| x.code.as_str().to_string())
        .collect();
    use thairag_agent::guardrails::GuardAction;
    match v.action {
        GuardAction::Pass => PreviewVerdict {
            action: "pass".into(),
            codes,
            output: None,
        },
        GuardAction::Sanitize(s) => PreviewVerdict {
            action: "sanitize".into(),
            codes,
            output: Some(s),
        },
        GuardAction::Block { reason } => PreviewVerdict {
            action: "block".into(),
            codes,
            output: Some(reason),
        },
        GuardAction::Regenerate { feedback } => PreviewVerdict {
            action: "regenerate".into(),
            codes,
            output: Some(feedback),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(input: Option<bool>, output: Option<bool>, codes: &str) -> InferenceLogEntry {
        InferenceLogEntry {
            id: "x".into(),
            timestamp: "2026-05-08T00:00:00Z".into(),
            user_id: None,
            workspace_id: None,
            org_id: None,
            dept_id: None,
            session_id: None,
            response_id: "r".into(),
            query_text: "q".into(),
            detected_language: None,
            intent: None,
            complexity: None,
            llm_kind: "ollama".into(),
            llm_model: "qwen3:4b".into(),
            settings_scope: "global".into(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_ms: 0,
            search_ms: None,
            generation_ms: None,
            chunks_retrieved: None,
            avg_chunk_score: None,
            self_rag_decision: None,
            self_rag_confidence: None,
            quality_guard_pass: None,
            relevance_score: None,
            hallucination_score: None,
            completeness_score: None,
            pipeline_route: None,
            agents_used: "[]".into(),
            status: "success".into(),
            error_message: None,
            response_length: 0,
            feedback_score: None,
            input_guardrails_pass: input,
            output_guardrails_pass: output,
            guardrail_violation_codes: codes.into(),
        }
    }

    #[test]
    fn aggregates_violation_counts() {
        let rows = vec![
            entry(Some(true), Some(true), ""),
            entry(Some(false), None, "PII_THAI_ID,PII_EMAIL"),
            entry(None, Some(false), "PII_EMAIL"),
        ];
        let s = aggregate_stats(&rows);
        assert_eq!(s.input_checks_total, 2);
        assert_eq!(s.output_checks_total, 2);
        assert_eq!(s.violations_total, 2);
        assert_eq!(s.input_blocks_total, 1);
        assert_eq!(s.output_blocks_total, 1);
        // PII_EMAIL appears twice; PII_THAI_ID once. Sorted desc.
        assert_eq!(s.by_code[0].code, "PII_EMAIL");
        assert_eq!(s.by_code[0].count, 2);
        assert_eq!(s.by_code[1].code, "PII_THAI_ID");
        assert_eq!(s.by_code[1].count, 1);
    }

    #[test]
    fn parse_codes_handles_empty() {
        assert!(parse_codes("").is_empty());
        assert_eq!(parse_codes("A,B"), vec!["A".to_string(), "B".to_string()]);
        assert_eq!(
            parse_codes(" A , B ,"),
            vec!["A".to_string(), "B".to_string()]
        );
    }
}
