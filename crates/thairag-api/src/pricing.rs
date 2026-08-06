//! Model pricing estimates (USD per 1M tokens).
//!
//! Single source of truth for cost estimation, shared by the Usage endpoint
//! and the inference-stats aggregations. These are ESTIMATES from a static
//! price table — the provider's own billing dashboard is the ground truth.
//! `Some(0.0)` = local/self-hosted (no per-token cost); `None` = unknown
//! model (cannot price).

/// Rough cost estimation based on known model pricing (USD per 1M tokens).
pub fn estimate_cost(kind: &str, model: &str, prompt: u64, completion: u64) -> Option<f64> {
    let (prompt_per_m, completion_per_m) = match kind {
        "claude" => match model {
            m if m.contains("opus") => (15.0, 75.0),
            m if m.contains("sonnet") => (3.0, 15.0),
            m if m.contains("haiku") => (0.25, 1.25),
            _ => (3.0, 15.0), // default sonnet pricing
        },
        "openai" | "open_ai" => match model {
            m if m.contains("gpt-4o-mini") => (0.15, 0.60),
            m if m.contains("gpt-4o") => (2.50, 10.0),
            m if m.contains("gpt-4-turbo") => (10.0, 30.0),
            m if m.contains("gpt-4") => (30.0, 60.0),
            m if m.contains("gpt-3.5") => (0.50, 1.50),
            m if m.contains("o1-mini") => (3.0, 12.0),
            m if m.contains("o1") => (15.0, 60.0),
            _ => return None,
        },
        "gemini" => match model {
            m if m.contains("pro") => (1.25, 5.0),
            m if m.contains("flash") => (0.075, 0.30),
            _ => (1.25, 5.0),
        },
        "ollama" | "open_ai_compatible" => return Some(0.0), // local — no cost
        _ => return None,
    };

    let cost = (prompt as f64 / 1_000_000.0) * prompt_per_m
        + (completion as f64 / 1_000_000.0) * completion_per_m;
    Some((cost * 10000.0).round() / 10000.0) // round to 4 decimals
}

#[cfg(test)]
mod tests {
    use super::estimate_cost;

    #[test]
    fn prices_known_cloud_models() {
        // 1M prompt + 1M completion at table rates
        assert_eq!(
            estimate_cost("open_ai", "gpt-4o-mini", 1_000_000, 1_000_000),
            Some(0.75)
        );
        assert_eq!(
            estimate_cost("openai", "gpt-4o-mini", 1_000_000, 0),
            Some(0.15)
        );
        assert_eq!(
            estimate_cost("claude", "claude-sonnet-4-20250514", 1_000_000, 1_000_000),
            Some(18.0)
        );
        assert_eq!(
            estimate_cost("gemini", "gemini-2.0-flash", 2_000_000, 0),
            Some(0.15)
        );
    }

    #[test]
    fn local_models_cost_zero_and_unknown_none() {
        assert_eq!(
            estimate_cost("ollama", "qwen3:14b", 5_000_000, 5_000_000),
            Some(0.0)
        );
        assert_eq!(
            estimate_cost("open_ai_compatible", "qwen3.6-27b-fast", 1, 1),
            Some(0.0)
        );
        assert_eq!(
            estimate_cost("openai", "totally-unknown-model", 1000, 1000),
            None
        );
        assert_eq!(estimate_cost("mystery_kind", "x", 1000, 1000), None);
    }

    #[test]
    fn rounds_to_four_decimals_and_zero_tokens_zero_cost() {
        assert_eq!(estimate_cost("openai", "gpt-4o-mini", 0, 0), Some(0.0));
        let c = estimate_cost("openai", "gpt-4o-mini", 123, 45).unwrap();
        assert_eq!(c, (c * 10000.0).round() / 10000.0);
    }
}
