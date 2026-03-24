use serde::{Deserialize, Deserializer};
use thairag_core::types::{
    EmbeddingKind, LlmKind, RerankerKind, TextSearchKind, VectorIsolation, VectorStoreKind,
};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub providers: ProvidersConfig,
    pub search: SearchConfig,
    pub document: DocumentConfig,
    #[serde(default)]
    pub chat_pipeline: ChatPipelineConfig,
    #[serde(default)]
    pub mcp: McpConfig,
}

impl AppConfig {
    pub fn validate(&self) -> std::result::Result<(), String> {
        let require = |field: &str, value: &str| -> std::result::Result<(), String> {
            if value.trim().is_empty() {
                Err(format!("{field} must not be empty"))
            } else {
                Ok(())
            }
        };

        let p = &self.providers;

        // LLM
        match p.llm.kind {
            LlmKind::Ollama => require("providers.llm.base_url", &p.llm.base_url)?,
            LlmKind::Claude | LlmKind::OpenAi | LlmKind::Gemini => {
                require("providers.llm.api_key", &p.llm.api_key)?
            }
            LlmKind::OpenAiCompatible => {
                require("providers.llm.base_url", &p.llm.base_url)?;
                require("providers.llm.api_key", &p.llm.api_key)?;
            }
        }

        // Embedding
        match p.embedding.kind {
            EmbeddingKind::Fastembed => {}
            EmbeddingKind::OpenAi | EmbeddingKind::Cohere => {
                require("providers.embedding.api_key", &p.embedding.api_key)?;
            }
            EmbeddingKind::Ollama => {
                require("providers.embedding.base_url", &p.embedding.base_url)?;
            }
        }

        // Vector store
        match p.vector_store.kind {
            VectorStoreKind::InMemory => {}
            VectorStoreKind::Qdrant
            | VectorStoreKind::ChromaDb
            | VectorStoreKind::Milvus
            | VectorStoreKind::Weaviate => {
                require("providers.vector_store.url", &p.vector_store.url)?;
                require(
                    "providers.vector_store.collection",
                    &p.vector_store.collection,
                )?;
            }
            VectorStoreKind::Pgvector => {
                require("providers.vector_store.url", &p.vector_store.url)?;
            }
            VectorStoreKind::Pinecone => {
                require("providers.vector_store.url", &p.vector_store.url)?;
                require("providers.vector_store.api_key", &p.vector_store.api_key)?;
            }
        }

        // Reranker
        match p.reranker.kind {
            RerankerKind::Passthrough => {}
            RerankerKind::Cohere | RerankerKind::Jina => {
                require("providers.reranker.api_key", &p.reranker.api_key)?;
                require("providers.reranker.model", &p.reranker.model)?;
            }
        }

        // LLM10: Validate chat pipeline budget caps
        let cp = &self.chat_pipeline;
        if cp.quality_guard_max_retries > 5 {
            return Err("chat_pipeline.quality_guard_max_retries must be <= 5".into());
        }
        if cp.refinement_max_retries > 5 {
            return Err("chat_pipeline.refinement_max_retries must be <= 5".into());
        }
        if cp.tool_use_max_calls > 10 {
            return Err("chat_pipeline.tool_use_max_calls must be <= 10".into());
        }
        if cp.max_llm_calls_per_request == 0 || cp.max_llm_calls_per_request > 50 {
            return Err("chat_pipeline.max_llm_calls_per_request must be 1..=50".into());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// Allowed CORS origins. Empty = permissive (dev only).
    /// Accepts either a YAML/TOML sequence or a comma-separated string
    /// (useful for env var overrides via config-rs).
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub cors_origins: Vec<String>,
    /// Trust X-Forwarded-For header from reverse proxy for client IP extraction.
    /// Only enable when running behind a trusted proxy (nginx, load balancer).
    #[serde(default)]
    pub trust_proxy: bool,
    /// Maximum number of messages in a single chat request.
    #[serde(default = "default_max_chat_messages")]
    pub max_chat_messages: usize,
    /// Maximum length (chars) of a single chat message content.
    #[serde(default = "default_max_message_length")]
    pub max_message_length: usize,
    /// Server-side request timeout (seconds) for non-streaming endpoints.
    /// Returns a proper JSON error before reverse proxy 504. Set higher than
    /// the proxy timeout to let the proxy handle it, or lower to return a
    /// helpful error message from ThaiRAG directly.
    #[serde(default = "default_server_request_timeout")]
    pub request_timeout_secs: u64,
}

fn default_max_chat_messages() -> usize {
    50
}

fn default_max_message_length() -> usize {
    32_000 // ~32K chars ≈ ~8K tokens
}

fn default_server_request_timeout() -> u64 {
    600 // 10 minutes — generous to cover cold model loading + multi-agent pipeline
}

fn default_shutdown_timeout() -> u64 {
    30
}

/// Deserialize a `Vec<String>` from either a YAML/TOML sequence **or** a
/// comma-separated string (the form config-rs produces from env vars).
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Vec(Vec<String>),
        String(String),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::Vec(v) => Ok(v),
        StringOrVec::String(s) if s.is_empty() => Ok(vec![]),
        StringOrVec::String(s) => Ok(s.split(',').map(|s| s.trim().to_string()).collect()),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_limit_enabled")]
    pub enabled: bool,
    #[serde(default = "default_requests_per_second")]
    pub requests_per_second: u64,
    #[serde(default = "default_burst_size")]
    pub burst_size: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_second: 10,
            burst_size: 20,
        }
    }
}

fn default_rate_limit_enabled() -> bool {
    true
}

fn default_requests_per_second() -> u64 {
    10
}

fn default_burst_size() -> u64 {
    20
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub jwt_secret: String,
    pub token_expiry_hours: u64,
    #[serde(default = "default_password_min_length")]
    pub password_min_length: usize,
    #[serde(default = "default_max_login_attempts")]
    pub max_login_attempts: u32,
    #[serde(default = "default_lockout_duration_secs")]
    pub lockout_duration_secs: u64,
    /// Static API keys accepted alongside JWT tokens.
    /// Comma-separated list, e.g. "sk-thairag-abc123,sk-thairag-xyz789"
    #[serde(default)]
    pub api_keys: String,
}

fn default_password_min_length() -> usize {
    8
}

fn default_max_login_attempts() -> u32 {
    5
}

fn default_lockout_duration_secs() -> u64 {
    300
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ProvidersConfig {
    pub llm: LlmConfig,
    pub embedding: EmbeddingConfig,
    pub vector_store: VectorStoreConfig,
    pub text_search: TextSearchConfig,
    pub reranker: RerankerConfig,
}

#[derive(Clone, Deserialize, serde::Serialize)]
pub struct LlmConfig {
    pub kind: LlmKind,
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    /// Per-agent max output tokens. None = use global `agent_max_tokens`.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Optional vault profile ID. When set, the profile's stored credentials
    /// are resolved at provider-build time via the Vault.
    #[serde(default)]
    pub profile_id: Option<String>,
}

impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("kind", &self.kind)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "(none)"
                } else {
                    "***"
                },
            )
            .field("max_tokens", &self.max_tokens)
            .field("profile_id", &self.profile_id)
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct EmbeddingConfig {
    pub kind: EmbeddingKind,
    pub model: String,
    pub dimension: usize,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct VectorStoreConfig {
    pub kind: VectorStoreKind,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub collection: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub isolation: VectorIsolation,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TextSearchConfig {
    pub kind: TextSearchKind,
    #[serde(default = "default_index_path")]
    pub index_path: String,
}

fn default_index_path() -> String {
    "./data/tantivy_index".to_string()
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct RerankerConfig {
    pub kind: RerankerKind,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SearchConfig {
    pub top_k: usize,
    pub rerank_top_k: usize,
    pub rrf_k: usize,
    pub vector_weight: f32,
    pub text_weight: f32,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct DocumentConfig {
    pub max_chunk_size: usize,
    pub chunk_overlap: usize,
    #[serde(default = "default_max_upload_size_mb")]
    pub max_upload_size_mb: usize,
    #[serde(default)]
    pub ai_preprocessing: AiPreprocessingConfig,
}

fn default_max_upload_size_mb() -> usize {
    50
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct AiPreprocessingConfig {
    /// Master switch. Default: false (mechanical pipeline).
    #[serde(default)]
    pub enabled: bool,
    /// When true (default), the Analyzer agent recommends quality_threshold,
    /// max_chunk_size, and min_ai_size dynamically per document.
    /// The values below become fallback defaults. When false, values are used as-is.
    #[serde(default = "default_true_val")]
    pub auto_params: bool,
    /// Minimum quality score (0.0..1.0) to accept AI conversion.
    #[serde(default = "default_quality_threshold")]
    pub quality_threshold: f32,
    /// Max input text length (chars) to send per LLM segment.
    #[serde(default = "default_max_llm_input_chars")]
    pub max_llm_input_chars: usize,
    /// Max tokens for each LLM agent call.
    #[serde(default = "default_agent_max_tokens")]
    pub agent_max_tokens: u32,
    /// Skip AI for files smaller than this (bytes).
    #[serde(default = "default_min_ai_size_bytes")]
    pub min_ai_size_bytes: usize,
    /// Shared LLM for all preprocessing agents (fallback: main chat LLM).
    #[serde(default)]
    pub llm: Option<LlmConfig>,
    /// Per-agent LLM overrides. Each falls back to `llm`, then to main chat LLM.
    #[serde(default)]
    pub analyzer_llm: Option<LlmConfig>,
    #[serde(default)]
    pub converter_llm: Option<LlmConfig>,
    #[serde(default)]
    pub quality_llm: Option<LlmConfig>,
    #[serde(default)]
    pub chunker_llm: Option<LlmConfig>,
    /// Retry-with-feedback settings.
    #[serde(default)]
    pub retry: AiRetryConfig,
    /// Enable LLM-driven orchestration (replaces hardcoded retry logic).
    #[serde(default)]
    pub orchestrator_enabled: bool,
    /// When true (default), budget is computed dynamically from document complexity.
    /// When false, uses `max_orchestrator_calls` as a fixed budget.
    #[serde(default = "default_true_val")]
    pub auto_orchestrator_budget: bool,
    /// Fixed budget when auto is off, or hard ceiling when auto is on.
    #[serde(default = "default_max_orchestrator_calls")]
    pub max_orchestrator_calls: u32,
    /// Separate LLM for the orchestrator agent.
    #[serde(default)]
    pub orchestrator_llm: Option<LlmConfig>,
    /// Enable chunk enrichment (context prefix, summary, keywords, HyDE queries).
    #[serde(default = "default_true_val")]
    pub enricher_enabled: bool,
    /// Separate LLM for the chunk enricher agent.
    #[serde(default)]
    pub enricher_llm: Option<LlmConfig>,
}

/// Controls retry-with-feedback behavior for AI agents.
/// When enabled, agents retry with quality feedback instead of immediately
/// falling back to mechanical processing.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct AiRetryConfig {
    /// Master switch. false = old behavior (fail → mechanical fallback).
    #[serde(default = "default_true_val")]
    pub enabled: bool,
    /// Max converter retries after quality check failure.
    #[serde(default = "default_converter_max_retries")]
    pub converter_max_retries: u32,
    /// Max chunker retries after validation failure.
    #[serde(default = "default_chunker_max_retries")]
    pub chunker_max_retries: u32,
    /// Max analyzer retries with larger excerpts on low confidence.
    #[serde(default = "default_analyzer_max_retries")]
    pub analyzer_max_retries: u32,
    /// Retry analyzer if confidence is below this (but above the 0.3 hard cutoff).
    #[serde(default = "default_analyzer_retry_confidence")]
    pub analyzer_retry_below_confidence: f32,
}

impl Default for AiRetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            converter_max_retries: default_converter_max_retries(),
            chunker_max_retries: default_chunker_max_retries(),
            analyzer_max_retries: default_analyzer_max_retries(),
            analyzer_retry_below_confidence: default_analyzer_retry_confidence(),
        }
    }
}

fn default_converter_max_retries() -> u32 {
    2
}
fn default_chunker_max_retries() -> u32 {
    1
}
fn default_analyzer_max_retries() -> u32 {
    1
}
fn default_analyzer_retry_confidence() -> f32 {
    0.5
}
fn default_max_orchestrator_calls() -> u32 {
    10
}

impl Default for AiPreprocessingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_params: true,
            quality_threshold: default_quality_threshold(),
            max_llm_input_chars: default_max_llm_input_chars(),
            agent_max_tokens: default_agent_max_tokens(),
            min_ai_size_bytes: default_min_ai_size_bytes(),
            llm: None,
            analyzer_llm: None,
            converter_llm: None,
            quality_llm: None,
            chunker_llm: None,
            retry: AiRetryConfig::default(),
            orchestrator_enabled: false,
            auto_orchestrator_budget: true,
            max_orchestrator_calls: default_max_orchestrator_calls(),
            orchestrator_llm: None,
            enricher_enabled: true,
            enricher_llm: None,
        }
    }
}

// ── Chat Pipeline Config ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ChatPipelineConfig {
    /// Master switch. When false, uses legacy 2-agent flow.
    #[serde(default)]
    pub enabled: bool,
    /// Shared LLM for all chat pipeline agents (fallback: main chat LLM).
    #[serde(default)]
    pub llm: Option<LlmConfig>,
    /// Per-agent toggles.
    #[serde(default = "default_true_val")]
    pub query_analyzer_enabled: bool,
    #[serde(default = "default_true_val")]
    pub query_rewriter_enabled: bool,
    #[serde(default = "default_true_val")]
    pub context_curator_enabled: bool,
    // Response Generator is always on (core agent).
    #[serde(default)]
    pub quality_guard_enabled: bool,
    #[serde(default = "default_true_val")]
    pub language_adapter_enabled: bool,
    /// Per-agent LLM overrides.
    #[serde(default)]
    pub query_analyzer_llm: Option<LlmConfig>,
    #[serde(default)]
    pub query_rewriter_llm: Option<LlmConfig>,
    #[serde(default)]
    pub context_curator_llm: Option<LlmConfig>,
    #[serde(default)]
    pub response_generator_llm: Option<LlmConfig>,
    #[serde(default)]
    pub quality_guard_llm: Option<LlmConfig>,
    #[serde(default)]
    pub language_adapter_llm: Option<LlmConfig>,
    /// Enable LLM-driven orchestration for dynamic agent routing.
    #[serde(default)]
    pub orchestrator_enabled: bool,
    /// Max LLM calls for orchestration per request.
    #[serde(default = "default_max_chat_orchestrator_calls")]
    pub max_orchestrator_calls: u32,
    /// Separate LLM for the pipeline orchestrator.
    #[serde(default)]
    pub orchestrator_llm: Option<LlmConfig>,
    /// Quality Guard: max retry attempts.
    #[serde(default = "default_quality_guard_max_retries")]
    pub quality_guard_max_retries: u32,
    /// Quality Guard: minimum relevance score to pass (0.0..1.0).
    #[serde(default = "default_quality_guard_threshold")]
    pub quality_guard_threshold: f32,
    /// Max estimated tokens for context window.
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    /// Max output tokens per agent call.
    #[serde(default = "default_chat_agent_max_tokens")]
    pub agent_max_tokens: u32,
    /// LLM10: Hard ceiling on total LLM calls per chat request (prevents cost explosion).
    #[serde(default = "default_max_llm_calls_per_request")]
    pub max_llm_calls_per_request: u32,
    /// Per-LLM-call timeout in seconds. Increase for large document sets or slow models.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    /// Ollama keep_alive duration. Controls how long models stay loaded in GPU memory.
    /// Examples: "5m" (default), "30m", "1h", "-1" (forever), "0" (unload immediately).
    #[serde(default = "default_ollama_keep_alive")]
    pub ollama_keep_alive: String,

    // ── Feature: Conversation Memory ──
    #[serde(default)]
    pub conversation_memory_enabled: bool,
    #[serde(default = "default_memory_max_summaries")]
    pub memory_max_summaries: usize,
    #[serde(default = "default_memory_summary_max_tokens")]
    pub memory_summary_max_tokens: u32,
    #[serde(default)]
    pub memory_llm: Option<LlmConfig>,

    // ── Feature: Multi-turn Retrieval Refinement ──
    #[serde(default)]
    pub retrieval_refinement_enabled: bool,
    #[serde(default = "default_refinement_min_relevance")]
    pub refinement_min_relevance: f32,
    #[serde(default = "default_refinement_max_retries")]
    pub refinement_max_retries: u32,

    // ── Feature: Agentic Tool Use ──
    #[serde(default)]
    pub tool_use_enabled: bool,
    #[serde(default = "default_tool_use_max_calls")]
    pub tool_use_max_calls: u32,
    #[serde(default)]
    pub tool_use_llm: Option<LlmConfig>,

    // ── Feature: Adaptive Quality Thresholds ──
    #[serde(default)]
    pub adaptive_threshold_enabled: bool,
    #[serde(default = "default_feedback_decay_days")]
    pub feedback_decay_days: u32,
    #[serde(default = "default_adaptive_min_samples")]
    pub adaptive_min_samples: u32,

    // ── Feature: Self-RAG ──
    #[serde(default)]
    pub self_rag_enabled: bool,
    #[serde(default = "default_self_rag_threshold")]
    pub self_rag_threshold: f32,
    #[serde(default)]
    pub self_rag_llm: Option<LlmConfig>,

    // ── Feature: Graph RAG ──
    #[serde(default)]
    pub graph_rag_enabled: bool,
    #[serde(default = "default_graph_rag_max_entities")]
    pub graph_rag_max_entities: u32,
    #[serde(default = "default_graph_rag_max_depth")]
    pub graph_rag_max_depth: u32,
    #[serde(default)]
    pub graph_rag_llm: Option<LlmConfig>,

    // ── Feature: Corrective RAG (CRAG) ──
    #[serde(default)]
    pub crag_enabled: bool,
    #[serde(default = "default_crag_relevance_threshold")]
    pub crag_relevance_threshold: f32,
    #[serde(default)]
    pub crag_web_search_url: String,
    #[serde(default = "default_crag_max_web_results")]
    pub crag_max_web_results: u32,

    // ── Feature: Speculative RAG ──
    #[serde(default)]
    pub speculative_rag_enabled: bool,
    #[serde(default = "default_speculative_candidates")]
    pub speculative_candidates: u32,

    // ── Feature: Map-Reduce RAG ──
    #[serde(default)]
    pub map_reduce_enabled: bool,
    #[serde(default = "default_map_reduce_max_chunks")]
    pub map_reduce_max_chunks: usize,
    #[serde(default)]
    pub map_reduce_llm: Option<LlmConfig>,

    // ── Feature: RAGAS Evaluation ──
    #[serde(default)]
    pub ragas_enabled: bool,
    #[serde(default = "default_ragas_sample_rate")]
    pub ragas_sample_rate: f32,
    #[serde(default)]
    pub ragas_llm: Option<LlmConfig>,

    // ── Feature: Contextual Compression (LLMLingua-style) ──
    #[serde(default)]
    pub compression_enabled: bool,
    #[serde(default = "default_compression_target_ratio")]
    pub compression_target_ratio: f32,
    #[serde(default)]
    pub compression_llm: Option<LlmConfig>,

    // ── Feature: Multi-modal RAG ──
    #[serde(default)]
    pub multimodal_enabled: bool,
    #[serde(default = "default_multimodal_max_images")]
    pub multimodal_max_images: u32,
    #[serde(default)]
    pub multimodal_llm: Option<LlmConfig>,

    // ── Feature: RAPTOR (Hierarchical Summaries) ──
    #[serde(default)]
    pub raptor_enabled: bool,
    #[serde(default = "default_raptor_max_depth")]
    pub raptor_max_depth: u32,
    #[serde(default = "default_raptor_group_size")]
    pub raptor_group_size: usize,
    #[serde(default)]
    pub raptor_llm: Option<LlmConfig>,

    // ── Feature: ColBERT Late Interaction Reranking ──
    #[serde(default)]
    pub colbert_enabled: bool,
    #[serde(default = "default_colbert_top_n")]
    pub colbert_top_n: usize,
    #[serde(default)]
    pub colbert_llm: Option<LlmConfig>,

    // ── Feature: Active Learning ──
    #[serde(default)]
    pub active_learning_enabled: bool,
    #[serde(default = "default_active_learning_min_interactions")]
    pub active_learning_min_interactions: u32,
    #[serde(default = "default_active_learning_max_low_confidence")]
    pub active_learning_max_low_confidence: usize,

    // ── Feature: Context Compaction ──
    /// Enable automatic context compaction when approaching model context limit.
    #[serde(default)]
    pub context_compaction_enabled: bool,
    /// Model context window size in tokens (0 = auto-detect from provider).
    #[serde(default = "default_model_context_window")]
    pub model_context_window: usize,
    /// Trigger compaction when context exceeds this fraction of the window (0.0–1.0).
    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: f32,
    /// Number of recent messages to keep intact during compaction.
    #[serde(default = "default_compaction_keep_recent")]
    pub compaction_keep_recent: usize,

    // ── Feature: Personal Memory (Per-User RAG) ──
    /// Enable vector-based personal memory per user.
    #[serde(default)]
    pub personal_memory_enabled: bool,
    /// Max personal memories to retrieve per query.
    #[serde(default = "default_personal_memory_top_k")]
    pub personal_memory_top_k: usize,
    /// Max personal memories stored per user (oldest pruned when exceeded).
    #[serde(default = "default_personal_memory_max_per_user")]
    pub personal_memory_max_per_user: usize,
    /// Daily relevance decay factor (0.0–1.0). Applied to relevance_score.
    #[serde(default = "default_personal_memory_decay_factor")]
    pub personal_memory_decay_factor: f32,
    /// Minimum relevance score before memory is pruned.
    #[serde(default = "default_personal_memory_min_relevance")]
    pub personal_memory_min_relevance: f32,
    /// Separate LLM for memory extraction (uses main LLM if not set).
    #[serde(default)]
    pub personal_memory_llm: Option<LlmConfig>,

    // ── Feature: Live Source Retrieval ──
    /// Enable live retrieval from MCP connectors when vector DB has no results.
    #[serde(default)]
    pub live_retrieval_enabled: bool,
    /// Overall timeout (seconds) for the live retrieval stage.
    #[serde(default = "default_live_retrieval_timeout_secs")]
    pub live_retrieval_timeout_secs: u64,
    /// Max number of MCP connectors to query in parallel.
    #[serde(default = "default_live_retrieval_max_connectors")]
    pub live_retrieval_max_connectors: u32,
    /// Max total characters of content to fetch from all connectors combined.
    #[serde(default = "default_live_retrieval_max_content_chars")]
    pub live_retrieval_max_content_chars: usize,
    /// Separate LLM for connector selection (uses main LLM if not set).
    #[serde(default)]
    pub live_retrieval_llm: Option<LlmConfig>,
}

fn default_max_chat_orchestrator_calls() -> u32 {
    3
}
fn default_quality_guard_max_retries() -> u32 {
    1
}
fn default_quality_guard_threshold() -> f32 {
    0.6
}
fn default_max_context_tokens() -> usize {
    4096
}
fn default_chat_agent_max_tokens() -> u32 {
    2048
}
fn default_max_llm_calls_per_request() -> u32 {
    12
}
fn default_request_timeout_secs() -> u64 {
    300
}
fn default_ollama_keep_alive() -> String {
    "5m".to_string()
}
fn default_memory_max_summaries() -> usize {
    10
}
fn default_memory_summary_max_tokens() -> u32 {
    256
}
fn default_refinement_min_relevance() -> f32 {
    0.3
}
fn default_refinement_max_retries() -> u32 {
    1
}
fn default_tool_use_max_calls() -> u32 {
    3
}
fn default_feedback_decay_days() -> u32 {
    30
}
fn default_adaptive_min_samples() -> u32 {
    20
}
fn default_self_rag_threshold() -> f32 {
    0.7
}
fn default_graph_rag_max_entities() -> u32 {
    10
}
fn default_graph_rag_max_depth() -> u32 {
    2
}
fn default_crag_relevance_threshold() -> f32 {
    0.3
}
fn default_crag_max_web_results() -> u32 {
    5
}
fn default_speculative_candidates() -> u32 {
    3
}
fn default_map_reduce_max_chunks() -> usize {
    15
}
fn default_ragas_sample_rate() -> f32 {
    0.1
}
fn default_compression_target_ratio() -> f32 {
    0.5
}
fn default_multimodal_max_images() -> u32 {
    5
}
fn default_raptor_max_depth() -> u32 {
    2
}
fn default_raptor_group_size() -> usize {
    3
}
fn default_colbert_top_n() -> usize {
    10
}
fn default_active_learning_min_interactions() -> u32 {
    5
}
fn default_active_learning_max_low_confidence() -> usize {
    100
}
fn default_model_context_window() -> usize {
    0 // 0 = auto-detect
}
fn default_compaction_threshold() -> f32 {
    0.8
}
fn default_compaction_keep_recent() -> usize {
    6
}
fn default_personal_memory_top_k() -> usize {
    5
}
fn default_personal_memory_max_per_user() -> usize {
    200
}
fn default_personal_memory_decay_factor() -> f32 {
    0.95
}
fn default_personal_memory_min_relevance() -> f32 {
    0.1
}
fn default_live_retrieval_timeout_secs() -> u64 {
    15
}
fn default_live_retrieval_max_connectors() -> u32 {
    3
}
fn default_live_retrieval_max_content_chars() -> usize {
    30_000
}

impl Default for ChatPipelineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            llm: None,
            query_analyzer_enabled: true,
            query_rewriter_enabled: true,
            context_curator_enabled: true,
            quality_guard_enabled: false,
            language_adapter_enabled: true,
            query_analyzer_llm: None,
            query_rewriter_llm: None,
            context_curator_llm: None,
            response_generator_llm: None,
            quality_guard_llm: None,
            language_adapter_llm: None,
            orchestrator_enabled: false,
            max_orchestrator_calls: default_max_chat_orchestrator_calls(),
            orchestrator_llm: None,
            quality_guard_max_retries: default_quality_guard_max_retries(),
            quality_guard_threshold: default_quality_guard_threshold(),
            max_context_tokens: default_max_context_tokens(),
            agent_max_tokens: default_chat_agent_max_tokens(),
            max_llm_calls_per_request: default_max_llm_calls_per_request(),
            request_timeout_secs: default_request_timeout_secs(),
            ollama_keep_alive: default_ollama_keep_alive(),
            conversation_memory_enabled: false,
            memory_max_summaries: default_memory_max_summaries(),
            memory_summary_max_tokens: default_memory_summary_max_tokens(),
            memory_llm: None,
            retrieval_refinement_enabled: false,
            refinement_min_relevance: default_refinement_min_relevance(),
            refinement_max_retries: default_refinement_max_retries(),
            tool_use_enabled: false,
            tool_use_max_calls: default_tool_use_max_calls(),
            tool_use_llm: None,
            adaptive_threshold_enabled: false,
            feedback_decay_days: default_feedback_decay_days(),
            adaptive_min_samples: default_adaptive_min_samples(),
            // Self-RAG
            self_rag_enabled: false,
            self_rag_threshold: default_self_rag_threshold(),
            self_rag_llm: None,
            // Graph RAG
            graph_rag_enabled: false,
            graph_rag_max_entities: default_graph_rag_max_entities(),
            graph_rag_max_depth: default_graph_rag_max_depth(),
            graph_rag_llm: None,
            // CRAG
            crag_enabled: false,
            crag_relevance_threshold: default_crag_relevance_threshold(),
            crag_web_search_url: String::new(),
            crag_max_web_results: default_crag_max_web_results(),
            // Speculative RAG
            speculative_rag_enabled: false,
            speculative_candidates: default_speculative_candidates(),
            // Map-Reduce RAG
            map_reduce_enabled: false,
            map_reduce_max_chunks: default_map_reduce_max_chunks(),
            map_reduce_llm: None,
            // RAGAS
            ragas_enabled: false,
            ragas_sample_rate: default_ragas_sample_rate(),
            ragas_llm: None,
            // Contextual Compression
            compression_enabled: false,
            compression_target_ratio: default_compression_target_ratio(),
            compression_llm: None,
            // Multi-modal RAG
            multimodal_enabled: false,
            multimodal_max_images: default_multimodal_max_images(),
            multimodal_llm: None,
            // RAPTOR
            raptor_enabled: false,
            raptor_max_depth: default_raptor_max_depth(),
            raptor_group_size: default_raptor_group_size(),
            raptor_llm: None,
            // ColBERT
            colbert_enabled: false,
            colbert_top_n: default_colbert_top_n(),
            colbert_llm: None,
            // Active Learning
            active_learning_enabled: false,
            active_learning_min_interactions: default_active_learning_min_interactions(),
            active_learning_max_low_confidence: default_active_learning_max_low_confidence(),
            // Context Compaction
            context_compaction_enabled: false,
            model_context_window: default_model_context_window(),
            compaction_threshold: default_compaction_threshold(),
            compaction_keep_recent: default_compaction_keep_recent(),
            // Personal Memory
            personal_memory_enabled: false,
            personal_memory_top_k: default_personal_memory_top_k(),
            personal_memory_max_per_user: default_personal_memory_max_per_user(),
            personal_memory_decay_factor: default_personal_memory_decay_factor(),
            personal_memory_min_relevance: default_personal_memory_min_relevance(),
            personal_memory_llm: None,
            // Live Source Retrieval
            live_retrieval_enabled: false,
            live_retrieval_timeout_secs: default_live_retrieval_timeout_secs(),
            live_retrieval_max_connectors: default_live_retrieval_max_connectors(),
            live_retrieval_max_content_chars: default_live_retrieval_max_content_chars(),
            live_retrieval_llm: None,
        }
    }
}

fn default_true_val() -> bool {
    true
}

// ── MCP Connector Config ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    /// Master switch to enable MCP connector functionality.
    #[serde(default)]
    pub enabled: bool,
    /// Maximum concurrent sync operations across all connectors.
    #[serde(default = "default_max_concurrent_syncs")]
    pub max_concurrent_syncs: usize,
    /// Timeout (seconds) for MCP server connections.
    #[serde(default = "default_mcp_connect_timeout")]
    pub connect_timeout_secs: u64,
    /// Timeout (seconds) for individual resource read operations.
    #[serde(default = "default_mcp_read_timeout")]
    pub read_timeout_secs: u64,
    /// Whether to generate LLM summaries during sync.
    #[serde(default)]
    pub summarize_on_sync: bool,
    /// Maximum content size in bytes to accept from MCP resources.
    #[serde(default = "default_max_resource_size")]
    pub max_resource_size_bytes: usize,
    /// Separate LLM for content summarization during sync.
    #[serde(default)]
    pub summarization_llm: Option<LlmConfig>,
    /// Max retry attempts for sync connect/discover phase.
    #[serde(default = "default_sync_retry_max_attempts")]
    pub sync_retry_max_attempts: u32,
    /// Base delay (seconds) for exponential backoff between retries.
    #[serde(default = "default_sync_retry_base_delay_secs")]
    pub sync_retry_base_delay_secs: u64,
    /// Maximum delay (seconds) cap for exponential backoff.
    #[serde(default = "default_sync_retry_max_delay_secs")]
    pub sync_retry_max_delay_secs: u64,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_concurrent_syncs: default_max_concurrent_syncs(),
            connect_timeout_secs: default_mcp_connect_timeout(),
            read_timeout_secs: default_mcp_read_timeout(),
            summarize_on_sync: false,
            max_resource_size_bytes: default_max_resource_size(),
            summarization_llm: None,
            sync_retry_max_attempts: default_sync_retry_max_attempts(),
            sync_retry_base_delay_secs: default_sync_retry_base_delay_secs(),
            sync_retry_max_delay_secs: default_sync_retry_max_delay_secs(),
        }
    }
}

fn default_max_concurrent_syncs() -> usize {
    3
}
fn default_mcp_connect_timeout() -> u64 {
    30
}
fn default_mcp_read_timeout() -> u64 {
    120
}
fn default_max_resource_size() -> usize {
    50 * 1024 * 1024 // 50MB
}
fn default_sync_retry_max_attempts() -> u32 {
    3
}
fn default_sync_retry_base_delay_secs() -> u64 {
    2
}
fn default_sync_retry_max_delay_secs() -> u64 {
    60
}

fn default_quality_threshold() -> f32 {
    0.7
}
fn default_max_llm_input_chars() -> usize {
    30_000
}
fn default_agent_max_tokens() -> u32 {
    4096
}
fn default_min_ai_size_bytes() -> usize {
    500
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{
        EmbeddingKind, LlmKind, RerankerKind, TextSearchKind, VectorIsolation, VectorStoreKind,
    };

    fn free_tier_config() -> AppConfig {
        AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 3000,
                shutdown_timeout_secs: 30,
                rate_limit: RateLimitConfig::default(),
                cors_origins: vec![],
                trust_proxy: false,
                max_chat_messages: default_max_chat_messages(),
                max_message_length: default_max_message_length(),
                request_timeout_secs: default_server_request_timeout(),
            },
            database: DatabaseConfig {
                url: "sqlite://data.db".into(),
                max_connections: 5,
            },
            auth: AuthConfig {
                enabled: false,
                jwt_secret: "secret".into(),
                token_expiry_hours: 24,
                password_min_length: 8,
                max_login_attempts: 5,
                lockout_duration_secs: 300,
                api_keys: String::new(),
            },
            providers: ProvidersConfig {
                llm: LlmConfig {
                    kind: LlmKind::Ollama,
                    model: "llama3".into(),
                    base_url: "http://localhost:11435".into(),
                    api_key: String::new(),
                    max_tokens: None,
                    profile_id: None,
                },
                embedding: EmbeddingConfig {
                    kind: EmbeddingKind::Fastembed,
                    model: "all-MiniLM-L6-v2".into(),
                    dimension: 384,
                    base_url: String::new(),
                    api_key: String::new(),
                },
                vector_store: VectorStoreConfig {
                    kind: VectorStoreKind::InMemory,
                    url: String::new(),
                    collection: String::new(),
                    api_key: String::new(),
                    isolation: VectorIsolation::Shared,
                },
                text_search: TextSearchConfig {
                    kind: TextSearchKind::Tantivy,
                    index_path: "./data/tantivy_index".into(),
                },
                reranker: RerankerConfig {
                    kind: RerankerKind::Passthrough,
                    model: String::new(),
                    api_key: String::new(),
                },
            },
            search: SearchConfig {
                top_k: 5,
                rerank_top_k: 3,
                rrf_k: 60,
                vector_weight: 0.5,
                text_weight: 0.5,
            },
            document: DocumentConfig {
                max_chunk_size: 512,
                chunk_overlap: 64,
                max_upload_size_mb: 50,
                ai_preprocessing: AiPreprocessingConfig::default(),
            },
            chat_pipeline: ChatPipelineConfig::default(),
            mcp: McpConfig::default(),
        }
    }

    #[test]
    fn validate_free_tier_ok() {
        assert!(free_tier_config().validate().is_ok());
    }

    #[test]
    fn validate_missing_llm_api_key() {
        let mut cfg = free_tier_config();
        cfg.providers.llm.kind = LlmKind::Claude;
        cfg.providers.llm.api_key = String::new();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.llm.api_key"), "got: {err}");
    }

    #[test]
    fn validate_missing_ollama_base_url() {
        let mut cfg = free_tier_config();
        cfg.providers.llm.base_url = "  ".into();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.llm.base_url"), "got: {err}");
    }

    #[test]
    fn validate_missing_qdrant_fields() {
        let mut cfg = free_tier_config();
        cfg.providers.vector_store.kind = VectorStoreKind::Qdrant;
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.vector_store.url"), "got: {err}");
    }

    #[test]
    fn validate_missing_cohere_api_key() {
        let mut cfg = free_tier_config();
        cfg.providers.reranker.kind = RerankerKind::Cohere;
        cfg.providers.reranker.model = "rerank-v3".into();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("providers.reranker.api_key"), "got: {err}");
    }
}
