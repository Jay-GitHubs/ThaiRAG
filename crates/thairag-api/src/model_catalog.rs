//! Layered model-capability catalog (the recommendation resolver).
//!
//! ADVISORY ONLY — this drives the ⭐ "recommended" and "vision" badges in the
//! admin UI. It never gates model selection: capability informs, never enforces
//! (see PR-A). Unknown models always stay usable.
//!
//! An admin picks one discovery source via `ModelDiscoveryConfig.mode`; its
//! results are cached and resolved per model id. Modes:
//!
//! - "catalog"      — LiteLLM `model_prices_and_context_window.json` (default)
//! - "http_catalog" — a custom HTTP endpoint returning a capability JSON
//! - "mcp"          — an MCP discovery tool (orchestrated in routes/settings)
//!
//! Anything not covered by the active source falls back to the built-in floor
//! (known vision families + a curated recommended shortlist) — the offline
//! default for air-gapped deploys or when a fetch fails.

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
/// `model_discovery` KM-store setting key.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelDiscoveryConfig {
    /// Master toggle. When false (air-gapped), only the built-in floor is used.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// External catalog URL (LiteLLM-format JSON). Empty → [`DEFAULT_CATALOG_URL`].
    #[serde(default)]
    pub catalog_url: String,
    /// Discovery mode: "catalog" (built-in LiteLLM) | "http_catalog" (custom
    /// HTTP endpoint) | "mcp" (MCP discovery tool).
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Endpoint for the mcp/http_catalog modes: a URL (http_catalog → JSON
    /// catalog; mcp → MCP server SSE/HTTP URL).
    #[serde(default)]
    pub endpoint: String,
    /// MCP tool name to call for capabilities (mcp mode). Empty → "list_models".
    #[serde(default)]
    pub tool: String,
    /// Optional bearer token sent to the http_catalog endpoint / MCP server.
    #[serde(default)]
    pub auth: String,
}

impl Default for ModelDiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            catalog_url: String::new(),
            mode: default_mode(),
            endpoint: String::new(),
            tool: String::new(),
            auth: String::new(),
        }
    }
}

impl ModelDiscoveryConfig {
    /// The LiteLLM catalog URL, falling back to the default when unset.
    pub fn effective_url(&self) -> &str {
        let trimmed = self.catalog_url.trim();
        if trimmed.is_empty() {
            DEFAULT_CATALOG_URL
        } else {
            trimmed
        }
    }

    /// The HTTP URL to fetch a capability JSON from for the current mode:
    /// `http_catalog` uses `endpoint` (falling back to the LiteLLM default),
    /// every other HTTP mode uses [`effective_url`](Self::effective_url).
    pub fn source_url(&self) -> &str {
        let endpoint = self.endpoint.trim();
        if self.mode == "http_catalog" && !endpoint.is_empty() {
            endpoint
        } else {
            self.effective_url()
        }
    }

    /// MCP tool name to invoke, defaulting to "list_models".
    pub fn mcp_tool(&self) -> &str {
        let trimmed = self.tool.trim();
        if trimmed.is_empty() {
            "list_models"
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
    /// Some(_) when a discovery source supplies it; None ⇒ defer to built-in.
    recommended: Option<bool>,
    max_input_tokens: Option<u64>,
}

/// A model row produced by any discovery source (LiteLLM, custom HTTP, or an
/// MCP tool) before it is folded into the catalog cache.
#[derive(Clone, Debug)]
pub struct DiscoveredModel {
    pub id: String,
    pub vision: bool,
    pub recommended: Option<bool>,
    pub max_input_tokens: Option<u64>,
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

    /// Claim the single-flight refresh slot. Returns false if a refresh is
    /// already running (the caller should skip).
    pub fn try_begin_refresh(&self) -> bool {
        self.refreshing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Release the single-flight refresh slot.
    pub fn end_refresh(&self) {
        self.refreshing.store(false, Ordering::SeqCst);
    }

    /// Replace the cache with a freshly discovered model set.
    pub fn apply(&self, models: Vec<DiscoveredModel>) {
        let (by_full, by_suffix, count) = build_maps(&models);
        let mut data = self.data.write().unwrap();
        data.by_full = by_full;
        data.by_suffix = by_suffix;
        data.model_count = count;
        data.fetched_at = Some(Instant::now());
        data.last_error = None;
    }

    /// Record a discovery failure (cache contents are left untouched).
    pub fn record_error(&self, err: String) {
        self.data.write().unwrap().last_error = Some(err);
    }

    /// Resolve a model's capabilities: catalog hit → discovery verdict (vision +
    /// context, and `recommended` if the source supplied it), otherwise the
    /// built-in floor. `recommended` defers to the built-in shortlist when no
    /// discovery source provides it.
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
                recommended: c.recommended.unwrap_or(recommended),
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

/// Fold a discovered model set into the by-full + by-suffix lookup maps.
fn build_maps(
    models: &[DiscoveredModel],
) -> (
    HashMap<String, CatalogCaps>,
    HashMap<String, CatalogCaps>,
    usize,
) {
    let mut by_full: HashMap<String, CatalogCaps> = HashMap::new();
    let mut by_suffix: HashMap<String, CatalogCaps> = HashMap::new();
    for m in models {
        let caps = CatalogCaps {
            vision: m.vision,
            recommended: m.recommended,
            max_input_tokens: m.max_input_tokens,
        };
        let k = m.id.to_lowercase();
        if let Some(suffix) = k.rsplit('/').next() {
            // First entry wins to avoid clobbering across providers.
            by_suffix
                .entry(suffix.to_string())
                .or_insert_with(|| caps.clone());
        }
        by_full.insert(k, caps);
    }
    let count = by_full.len();
    (by_full, by_suffix, count)
}

/// Fetch a capability JSON over HTTP (LiteLLM catalog or a custom http_catalog
/// endpoint) and flexibly parse it. `auth`, when non-empty, is sent as a bearer
/// token. Errors are returned (never panic) so the resolver degrades to the floor.
pub async fn fetch_http(url: &str, auth: &str) -> Result<Vec<DiscoveredModel>, String> {
    let client = reqwest::Client::new();
    let mut req = client.get(url).timeout(Duration::from_secs(15));
    if !auth.trim().is_empty() {
        req = req.bearer_auth(auth.trim());
    }
    let resp = req
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
    let models = parse_capability_json(&body);
    if models.is_empty() {
        return Err("catalog returned no recognizable models".to_string());
    }
    Ok(models)
}

/// Read vision/recommended/context from a JSON object's fields, tolerating both
/// LiteLLM's `supports_vision` and a plain `vision` key.
fn caps_from_object(
    o: &serde_json::Map<String, serde_json::Value>,
) -> (bool, Option<bool>, Option<u64>) {
    let vision = o
        .get("supports_vision")
        .or_else(|| o.get("vision"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let recommended = o.get("recommended").and_then(|v| v.as_bool());
    let max_input_tokens = o.get("max_input_tokens").and_then(|v| v.as_u64());
    (vision, recommended, max_input_tokens)
}

/// Flexibly parse a capability payload into discovered models. Accepts:
///
/// - a LiteLLM-style object map `{ "<id>": { supports_vision, ... }, ... }`
/// - an array of model objects `[{ id|model, vision|supports_vision, ... }]`
/// - an array that wraps either of the above (e.g. an MCP tool result).
///
/// `sample_spec` (LiteLLM's descriptor entry) is skipped.
pub fn parse_capability_json(value: &serde_json::Value) -> Vec<DiscoveredModel> {
    let mut out = Vec::new();
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "sample_spec" {
                    continue;
                }
                if let Some(o) = val.as_object() {
                    let (vision, recommended, max_input_tokens) = caps_from_object(o);
                    out.push(DiscoveredModel {
                        id: key.clone(),
                        vision,
                        recommended,
                        max_input_tokens,
                    });
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for el in arr {
                if let Some(o) = el.as_object() {
                    match o
                        .get("id")
                        .or_else(|| o.get("model"))
                        .and_then(|v| v.as_str())
                    {
                        Some(id) => {
                            let (vision, recommended, max_input_tokens) = caps_from_object(o);
                            out.push(DiscoveredModel {
                                id: id.to_string(),
                                vision,
                                recommended,
                                max_input_tokens,
                            });
                        }
                        // A wrapper object (e.g. {"models": {...}}) — recurse.
                        None => out.extend(parse_capability_json(el)),
                    }
                }
            }
        }
        _ => {}
    }
    out
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
        cat.apply(vec![DiscoveredModel {
            id: "gpt-4o".into(),
            vision: true,
            recommended: None,
            max_input_tokens: Some(128_000),
        }]);
        let caps = cat.resolve(&LlmKind::OpenAi, "gpt-4o");
        assert_eq!(caps.source, "catalog");
        assert!(caps.vision);
        assert_eq!(caps.max_input_tokens, Some(128_000));
        // recommended not supplied by the source → built-in floor (gpt-4o is on it)
        assert!(caps.recommended);
    }

    #[test]
    fn discovery_can_override_recommended() {
        let cat = ModelCatalog::new();
        cat.apply(vec![DiscoveredModel {
            id: "some-obscure-model".into(),
            vision: false,
            recommended: Some(true),
            max_input_tokens: None,
        }]);
        let caps = cat.resolve(&LlmKind::OpenAiCompatible, "some-obscure-model");
        assert!(caps.recommended, "explicit discovery recommended=true wins");
    }

    #[test]
    fn parse_litellm_object_map() {
        let json = serde_json::json!({
            "sample_spec": { "note": "ignored" },
            "gpt-4o": { "supports_vision": true, "max_input_tokens": 128000 },
            "gemini/gemini-1.5-pro": { "vision": true, "recommended": false },
        });
        let models = parse_capability_json(&json);
        assert_eq!(models.len(), 2);
        let g = models.iter().find(|m| m.id == "gpt-4o").unwrap();
        assert!(g.vision);
        assert_eq!(g.max_input_tokens, Some(128000));
        let gem = models.iter().find(|m| m.id.contains("gemini")).unwrap();
        assert_eq!(gem.recommended, Some(false));
    }

    #[test]
    fn parse_array_of_models() {
        let json = serde_json::json!([
            { "id": "llava:13b", "vision": true, "recommended": true },
            { "model": "phi4", "supports_vision": false },
        ]);
        let models = parse_capability_json(&json);
        assert_eq!(models.len(), 2);
        assert!(models[0].vision);
        assert_eq!(models[0].recommended, Some(true));
        assert_eq!(models[1].id, "phi4");
    }

    #[test]
    fn source_url_prefers_endpoint_for_http_catalog() {
        let cfg = ModelDiscoveryConfig {
            mode: "http_catalog".into(),
            endpoint: "https://example.com/models.json".into(),
            ..Default::default()
        };
        assert_eq!(cfg.source_url(), "https://example.com/models.json");
        let cfg = ModelDiscoveryConfig {
            mode: "catalog".into(),
            endpoint: "https://ignored".into(),
            ..Default::default()
        };
        assert_eq!(cfg.source_url(), DEFAULT_CATALOG_URL);
    }
}
