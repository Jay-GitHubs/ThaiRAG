//! Silent-degradation telemetry.
//!
//! Every runtime agent that falls back to a heuristic after an LLM JSON
//! parse/call failure records the event here, turning what used to be only a
//! `warn!` log line into a counter the API layer exposes at `/metrics`
//! (`agent_json_fallback_total{agent=...}`). A rising count means the
//! configured model is failing the agent's JSON contract and answers are
//! being produced by degraded heuristics.

use std::collections::BTreeMap;
use std::sync::Mutex;

static FALLBACKS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

/// Record one degraded fallback for `agent` (static label, e.g. "query_analyzer").
pub fn record_fallback(agent: &'static str) {
    if let Ok(mut m) = FALLBACKS.lock() {
        *m.entry(agent).or_insert(0) += 1;
    }
}

/// Cumulative fallback counts per agent since process start.
pub fn fallback_counts() -> Vec<(&'static str, u64)> {
    FALLBACKS
        .lock()
        .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_reports() {
        record_fallback("test_agent");
        record_fallback("test_agent");
        let counts = fallback_counts();
        let n = counts.iter().find(|(a, _)| *a == "test_agent").unwrap().1;
        assert!(n >= 2);
    }
}
