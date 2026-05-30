//! Layered model-capability catalog (the recommendation resolver).
//!
//! ADVISORY ONLY — this drives the ⭐ "recommended" and "vision" badges in the
//! admin UI. It never gates model selection: capability informs, never enforces
//! (see PR-A). Unknown models always stay usable.
//!
//! Resolution order per model id:
//!   1. (PR-D2) admin-configured discovery tool — MCP / HTTP. Not yet wired.
//!   2. external catalog — LiteLLM `model_prices_and_context_window.json`,
//!      fetched over HTTP and cached with a daily TTL + background refresh.
//!   3. built-in floor — known vision families + a curated recommended shortlist
//!      (the offline floor, used for air-gapped deploys or when a fetch fails).

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thairag_core::types::LlmKind;

/// LiteLLM's public capability catalog (has `supports_vision`, context, pricing).
pub const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// How long a fetched catalog is considered fresh before a background refresh.
const CATALOG_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn default_true() -> bool {
    true
}

fn default_mode() -> String {
    "catalog".to_string()
}

/// Admin-configurable model-discovery settings, persisted under the
/// `model_discovery` KM-store setting key. `mode`/`endpoint` are reserved for
/// the PR-D2 MCP / HTTP discovery tool; PR-D1 uses the external catalog with an
/// enable toggle + optional URL override.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelDiscoveryConfig {
    /// Master toggle. When false (air-gapped), only the built-in floor is used.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// External catalog URL (LiteLLM-format JSON). Empty → [`DEFAULT_CATALOG_URL`].
    #[serde(default)]
    pub catalog_url: String,
    /// Discovery mode: "catalog" (PR-D1) | "mcp" | "http_catalog" (PR-D2).
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Endpoint for the mcp/http_catalog modes (PR-D2).
    #[serde(default)]
    pub endpoint: String,
}

impl Default for ModelDiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            catalog_url: String::new(),
            mode: default_mode(),
            endpoint: String::new(),
        }
    }
}

impl ModelDiscoveryConfig {
    /// The catalog URL to fetch, falling back to the default when unset.
    pub fn effective_url(&self) -> &str {
        let trimmed = self.catalog_url.trim();
        if trimmed.is_empty() {
            DEFAULT_CATALOG_URL
        } else {
            trimmed
        }
    }
}

/// Resolved capabilities for a single model id.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelCapabilities {
    /// Whether the model can accept image input.
    pub vision: bool,
    /// Whether the model is on the recommended shortlist.
    pub recommended: bool,
    /// Max input context tokens, when the catalog knows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// Where this verdict came from: "catalog" | "builtin".
    pub source: &'static str,
}

/// Public status of the cached catalog (drives the admin-UI banner).
#[derive(Clone, Debug, Serialize)]
pub struct CatalogStatus {
    pub has_data: bool,
    pub model_count: usize,
    /// Seconds since the last successful fetch, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<u64>,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Default)]
struct CatalogCaps {
    vision: bool,
    max_input_tokens: Option<u64>,
}

#[derive(Default)]
struct CatalogData {
    /// Lowercased full catalog key → caps (e.g. "gemini/gemini-1.5-pro").
    by_full: HashMap<String, CatalogCaps>,
    /// Lowercased key suffix after the last '/' → caps (e.g. "gemini-1.5-pro").
    by_suffix: HashMap<String, CatalogCaps>,
    fetched_at: Option<Instant>,
    model_count: usize,
    last_error: Option<String>,
}

/// Thread-safe, cached model-capability catalog.
pub struct ModelCatalog {
    data: RwLock<CatalogData>,
    refreshing: AtomicBool,
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelCatalog {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(CatalogData::default()),
            refreshing: AtomicBool::new(false),
        }
    }

    /// True when there is no cached data or it has aged past the TTL.
    pub fn is_stale(&self) -> bool {
        let data = self.data.read().unwrap();
        match data.fetched_at {
            None => true,
            Some(t) => t.elapsed() >= CATALOG_TTL,
        }
    }

    /// Snapshot of the current cache state for the admin-UI banner.
    pub fn status(&self) -> CatalogStatus {
        let data = self.data.read().unwrap();
        let age_secs = data.fetched_at.map(|t| t.elapsed().as_secs());
        let stale = match data.fetched_at {
            None => true,
            Some(t) => t.elapsed() >= CATALOG_TTL,
        };
        CatalogStatus {
            has_data: data.model_count > 0,
            model_count: data.model_count,
            age_secs,
            stale,
            last_error: data.last_error.clone(),
        }
    }

    /// Clear cached data (e.g. when discovery is disabled for air-gapped use).
    pub fn clear(&self) {
        let mut data = self.data.write().unwrap();
        *data = CatalogData::default();
    }

    /// Fetch + parse the external catalog, replacing the cache on success.
    /// Network errors are recorded (not propagated as panics) so the resolver
    /// gracefully degrades to the built-in floor. Returns the model count.
    pub async fn refresh(&self, url: &str) -> Result<usize, String> {
        // Single-flight: skip if another refresh is already in progress.
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(self.data.read().unwrap().model_count);
        }

        let result = self.fetch_and_parse(url).await;

        match &result {
            Ok((by_full, by_suffix, count)) => {
                let mut data = self.data.write().unwrap();
                data.by_full = by_full.clone();
                data.by_suffix = by_suffix.clone();
                data.model_count = *count;
                data.fetched_at = Some(Instant::now());
                data.last_error = None;
            }
            Err(e) => {
                let mut data = self.data.write().unwrap();
                data.last_error = Some(e.clone());
            }
        }

        self.refreshing.store(false, Ordering::SeqCst);
        result.map(|(_, _, count)| count)
    }

    async fn fetch_and_parse(
        &self,
        url: &str,
    ) -> Result<
        (
            HashMap<String, CatalogCaps>,
            HashMap<String, CatalogCaps>,
            usize,
        ),
        String,
    > {
        let client = reqwest::Client::new();
        let resp = client
            .get(url)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("catalog request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("catalog returned HTTP {}", resp.status()));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("catalog parse failed: {e}"))?;
        let obj = body
            .as_object()
            .ok_or_else(|| "catalog root is not a JSON object".to_string())?;

        let mut by_full: HashMap<String, CatalogCaps> = HashMap::new();
        let mut by_suffix: HashMap<String, CatalogCaps> = HashMap::new();
        for (key, val) in obj {
            // LiteLLM ships a non-model "sample_spec" descriptor entry.
            if key == "sample_spec" {
                continue;
            }
            let Some(o) = val.as_object() else { continue };
            let caps = CatalogCaps {
                vision: o
                    .get("supports_vision")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                max_input_tokens: o.get("max_input_tokens").and_then(|v| v.as_u64()),
            };
            let k = key.to_lowercase();
            if let Some(suffix) = k.rsplit('/').next() {
                // First entry wins to avoid clobbering across providers.
                by_suffix
                    .entry(suffix.to_string())
                    .or_insert_with(|| caps.clone());
            }
            by_full.insert(k, caps);
        }
        let count = by_full.len();
        Ok((by_full, by_suffix, count))
    }

    /// Resolve a model's capabilities: catalog hit → catalog verdict (vision +
    /// context), otherwise the built-in floor. `recommended` always comes from
    /// the built-in shortlist (the external catalog has no such notion; PR-D2's
    /// discovery tool can override it).
    pub fn resolve(&self, kind: &LlmKind, id: &str) -> ModelCapabilities {
        let recommended = builtin_recommended(kind, id);
        let norm = normalize_id(kind, id);

        let data = self.data.read().unwrap();
        let hit = data
            .by_full
            .get(&norm)
            .or_else(|| norm.rsplit('/').next().and_then(|s| data.by_suffix.get(s)))
            .or_else(|| data.by_suffix.get(&norm));

        match hit {
            Some(c) => ModelCapabilities {
                vision: c.vision,
                recommended,
                max_input_tokens: c.max_input_tokens,
                source: "catalog",
            },
            None => ModelCapabilities {
                vision: builtin_vision(kind, id),
                recommended,
                max_input_tokens: None,
                source: "builtin",
            },
        }
    }
}

/// Normalize a model id for catalog lookup: lowercase, and for Ollama strip the
/// `:tag` suffix (e.g. "qwen3-vl:8b-instruct" → "qwen3-vl").
fn normalize_id(kind: &LlmKind, id: &str) -> String {
    let lower = id.to_lowercase();
    match kind {
        LlmKind::Ollama | LlmKind::OpenAiCompatible => {
            lower.split(':').next().unwrap_or(&lower).to_string()
        }
        _ => lower,
    }
}

/// Built-in vision floor. Mirrors the runtime/admin check so the offline floor
/// never contradicts the provider's own `supports_vision()`.
fn builtin_vision(kind: &LlmKind, id: &str) -> bool {
    let m = id.to_lowercase();
    match kind {
        LlmKind::Claude => {
            m.contains("claude-3")
                || m.contains("claude-opus-4")
                || m.contains("claude-sonnet-4")
                || m.contains("claude-haiku-4")
        }
        LlmKind::OpenAi | LlmKind::OpenAiCompatible => {
            m.contains("gpt-4o")
                || m.contains("gpt-4.1")
                || m.contains("gpt-4-vision")
                || m.starts_with("o3")
                || m.starts_with("o4")
        }
        LlmKind::Gemini => m.contains("gemini-1.5") || m.contains("gemini-2"),
        LlmKind::Ollama => thairag_provider_llm::ollama::is_ollama_vision_model(id),
    }
}

/// Built-in recommended shortlist — current-generation, well-supported families.
/// Advisory; PR-D2's discovery tool can enrich/override this.
fn builtin_recommended(kind: &LlmKind, id: &str) -> bool {
    let m = id.to_lowercase();
    match kind {
        LlmKind::Claude => {
            m.contains("claude-3")
                || m.contains("opus-4")
                || m.contains("sonnet-4")
                || m.contains("haiku-4")
        }
        LlmKind::OpenAi => ["gpt-4o", "gpt-4.1", "o3", "o4"]
            .iter()
            .any(|f| m.contains(f)),
        LlmKind::OpenAiCompatible => false,
        LlmKind::Gemini => m.contains("gemini-1.5") || m.contains("gemini-2"),
        LlmKind::Ollama => {
            let base = m.split(':').next().unwrap_or(&m);
            [
                "qwen3",
                "qwen2.5",
                "qwen2.5vl",
                "llama3",
                "llama4",
                "gemma3",
                "llava",
                "iapp/chinda",
                "mistral",
                "phi4",
                "deepseek",
            ]
            .iter()
            .any(|f| {
                base == *f
                    || base.starts_with(&format!("{f}-"))
                    || base.starts_with(&format!("{f}:"))
                    || base.starts_with(f)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_catalog_falls_back_to_builtin() {
        let cat = ModelCatalog::new();
        // Unknown-to-catalog but known vision family.
        let caps = cat.resolve(&LlmKind::Ollama, "qwen3-vl:8b-instruct");
        assert!(caps.vision, "qwen3-vl should be vision via built-in floor");
        assert_eq!(caps.source, "builtin");
    }

    #[test]
    fn builtin_recommended_claude() {
        let cat = ModelCatalog::new();
        let caps = cat.resolve(&LlmKind::Claude, "claude-sonnet-4-20250514");
        assert!(caps.recommended);
        assert!(caps.vision);
    }

    #[test]
    fn discovery_config_default_url() {
        let cfg = ModelDiscoveryConfig::default();
        assert_eq!(cfg.effective_url(), DEFAULT_CATALOG_URL);
        let cfg = ModelDiscoveryConfig {
            catalog_url: "  ".into(),
            ..Default::default()
        };
        assert_eq!(cfg.effective_url(), DEFAULT_CATALOG_URL);
    }

    #[test]
    fn stale_when_empty() {
        let cat = ModelCatalog::new();
        assert!(cat.is_stale());
        let st = cat.status();
        assert!(!st.has_data);
        assert!(st.stale);
    }

    #[test]
    fn catalog_hit_overrides_builtin_vision() {
        let cat = ModelCatalog::new();
        {
            let mut data = cat.data.write().unwrap();
            data.by_full.insert(
                "gpt-4o".to_string(),
                CatalogCaps {
                    vision: true,
                    max_input_tokens: Some(128_000),
                },
            );
            data.model_count = 1;
            data.fetched_at = Some(Instant::now());
        }
        let caps = cat.resolve(&LlmKind::OpenAi, "gpt-4o");
        assert_eq!(caps.source, "catalog");
        assert!(caps.vision);
        assert_eq!(caps.max_input_tokens, Some(128_000));
    }
}
