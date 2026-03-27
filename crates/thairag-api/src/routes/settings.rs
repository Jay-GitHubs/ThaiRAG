use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::models::IdentityProvider;
use thairag_core::prompt_registry::PromptSource;
use thairag_core::types::IdpId;

use crate::app_state::AppState;
use crate::audit::{self, AuditEntry};
use crate::error::{ApiError, AppJson};
use crate::store::SettingsScope;

/// Query parameter for scoped settings endpoints.
#[derive(Deserialize, Default, Debug)]
pub struct ScopeQuery {
    pub scope_type: Option<String>,
    pub scope_id: Option<String>,
}

/// Parse a ScopeQuery into a SettingsScope, validating that scope_id exists.
fn parse_scope_query(
    sq: &ScopeQuery,
    store: &dyn crate::store::KmStoreTrait,
) -> Result<SettingsScope, ApiError> {
    use thairag_core::types::{DeptId, OrgId, WorkspaceId};

    match (sq.scope_type.as_deref(), sq.scope_id.as_deref()) {
        (None, _) | (Some("global"), _) => Ok(SettingsScope::Global),
        (Some("org"), Some(id)) => {
            let org_id = OrgId(
                id.parse()
                    .map_err(|_| ApiError(ThaiRagError::Validation("Invalid org UUID".into())))?,
            );
            store.get_org(org_id)?;
            Ok(SettingsScope::Org(org_id))
        }
        (Some("dept"), Some(id)) => {
            let dept_id = DeptId(
                id.parse()
                    .map_err(|_| ApiError(ThaiRagError::Validation("Invalid dept UUID".into())))?,
            );
            let dept = store.get_dept(dept_id)?;
            Ok(SettingsScope::Dept {
                org_id: dept.org_id,
                dept_id,
            })
        }
        (Some("workspace"), Some(id)) => {
            let ws_id = WorkspaceId(id.parse().map_err(|_| {
                ApiError(ThaiRagError::Validation("Invalid workspace UUID".into()))
            })?);
            let ws = store.get_workspace(ws_id)?;
            let dept = store.get_dept(ws.dept_id)?;
            Ok(SettingsScope::Workspace {
                org_id: dept.org_id,
                dept_id: ws.dept_id,
                workspace_id: ws_id,
            })
        }
        (Some(st), _) => Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid scope_type: {st}. Use 'global', 'org', 'dept', or 'workspace'"
        )))),
    }
}

/// Parse an LLM kind string accepting both PascalCase (from UI) and snake_case (from serde).
pub fn parse_llm_kind(s: &str) -> Result<thairag_core::types::LlmKind, String> {
    use thairag_core::types::LlmKind;
    match s {
        "Ollama" | "ollama" => Ok(LlmKind::Ollama),
        "Claude" | "claude" => Ok(LlmKind::Claude),
        "OpenAi" | "open_ai" | "openai" => Ok(LlmKind::OpenAi),
        "OpenAiCompatible" | "open_ai_compatible" => Ok(LlmKind::OpenAiCompatible),
        "Gemini" | "gemini" => Ok(LlmKind::Gemini),
        _ => Err(format!("Invalid LLM kind: {s}")),
    }
}

/// Parse an embedding kind string accepting both PascalCase (from UI) and snake_case (from serde).
fn parse_embedding_kind(s: &str) -> Result<thairag_core::types::EmbeddingKind, String> {
    use thairag_core::types::EmbeddingKind;
    match s {
        "Fastembed" | "fastembed" => Ok(EmbeddingKind::Fastembed),
        "Ollama" | "ollama" => Ok(EmbeddingKind::Ollama),
        "OpenAi" | "open_ai" | "openai" => Ok(EmbeddingKind::OpenAi),
        "Cohere" | "cohere" => Ok(EmbeddingKind::Cohere),
        _ => Err(format!("Invalid embedding kind: {s}")),
    }
}

/// Return the Debug (PascalCase) representation of a kind enum for API responses.
/// We use Debug format for consistency with the frontend's existing PascalCase values.
fn kind_str(kind: &impl std::fmt::Debug) -> String {
    format!("{kind:?}")
}

fn parse_vector_store_kind(s: &str) -> Result<thairag_core::types::VectorStoreKind, String> {
    use thairag_core::types::VectorStoreKind;
    match s {
        "InMemory" | "in_memory" => Ok(VectorStoreKind::InMemory),
        "Qdrant" | "qdrant" => Ok(VectorStoreKind::Qdrant),
        "Pgvector" | "pgvector" => Ok(VectorStoreKind::Pgvector),
        "ChromaDb" | "chroma_db" => Ok(VectorStoreKind::ChromaDb),
        "Pinecone" | "pinecone" => Ok(VectorStoreKind::Pinecone),
        "Weaviate" | "weaviate" => Ok(VectorStoreKind::Weaviate),
        "Milvus" | "milvus" => Ok(VectorStoreKind::Milvus),
        _ => Err(format!("Invalid vector store kind: {s}")),
    }
}

fn parse_reranker_kind(s: &str) -> Result<thairag_core::types::RerankerKind, String> {
    use thairag_core::types::RerankerKind;
    match s {
        "Passthrough" | "passthrough" => Ok(RerankerKind::Passthrough),
        "Cohere" | "cohere" => Ok(RerankerKind::Cohere),
        "Jina" | "jina" => Ok(RerankerKind::Jina),
        _ => Err(format!("Invalid reranker kind: {s}")),
    }
}

fn parse_vector_isolation(s: &str) -> Result<thairag_core::types::VectorIsolation, String> {
    use thairag_core::types::VectorIsolation;
    match s {
        "Shared" | "shared" => Ok(VectorIsolation::Shared),
        "PerOrganization" | "per_organization" => Ok(VectorIsolation::PerOrganization),
        "PerWorkspace" | "per_workspace" => Ok(VectorIsolation::PerWorkspace),
        _ => Err(format!("Invalid vector isolation: {s}")),
    }
}

// ── Provider config DTOs ────────────────────────────────────────────

#[derive(Serialize)]
pub struct ProviderConfigResponse {
    pub llm: LlmProviderInfo,
    pub embedding: EmbeddingProviderInfo,
    pub vector_store: VectorStoreProviderInfo,
    pub text_search: TextSearchProviderInfo,
    pub reranker: RerankerProviderInfo,
}

#[derive(Serialize)]
pub struct LlmProviderInfo {
    pub kind: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub has_api_key: bool,
    pub supports_vision: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Serialize)]
pub struct EmbeddingProviderInfo {
    pub kind: String,
    pub model: String,
    pub dimension: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub has_api_key: bool,
}

#[derive(Serialize)]
pub struct VectorStoreProviderInfo {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    pub has_api_key: bool,
    pub isolation: String,
}

#[derive(Serialize)]
pub struct TextSearchProviderInfo {
    pub kind: String,
    pub index_path: String,
}

#[derive(Serialize)]
pub struct RerankerProviderInfo {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub has_api_key: bool,
}

#[derive(Serialize)]
pub struct AvailableModel {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

#[derive(Serialize)]
pub struct ModelsResponse {
    pub provider: String,
    pub models: Vec<AvailableModel>,
}

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateIdpRequest {
    pub name: String,
    pub provider_type: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub config: serde_json::Value,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct UpdateIdpRequest {
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Serialize)]
pub struct IdpListResponse {
    pub data: Vec<IdentityProvider>,
    pub total: usize,
}

#[derive(Serialize)]
pub struct PublicIdpInfo {
    pub id: String,
    pub name: String,
    pub provider_type: String,
}

#[derive(Serialize)]
pub struct TestConnectionResponse {
    pub success: bool,
    pub message: String,
}

// ── Helpers ─────────────────────────────────────────────────────────

pub fn require_super_admin(claims: &AuthClaims, state: &AppState) -> Result<(), ApiError> {
    if claims.sub == "anonymous" {
        return Ok(());
    }
    let Ok(user_id) = claims.sub.parse::<Uuid>() else {
        return Err(ApiError(ThaiRagError::Authorization("Invalid user".into())));
    };
    let user = state
        .km_store
        .get_user(thairag_core::types::UserId(user_id))?;
    if !user.is_super_admin {
        return Err(ApiError(ThaiRagError::Authorization(
            "Super admin access required".into(),
        )));
    }
    Ok(())
}

// ── Protected endpoints (super admin only) ──────────────────────────

pub async fn list_identity_providers(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<IdpListResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    let providers = state.km_store.list_identity_providers();
    let total = providers.len();
    Ok(Json(IdpListResponse {
        data: providers,
        total,
    }))
}

pub async fn create_identity_provider(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<CreateIdpRequest>,
) -> Result<(StatusCode, Json<IdentityProvider>), ApiError> {
    require_super_admin(&claims, &state)?;

    let valid_types = ["oidc", "oauth2", "saml", "ldap"];
    if !valid_types.contains(&body.provider_type.as_str()) {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid provider_type '{}'. Must be one of: {}",
            body.provider_type,
            valid_types.join(", ")
        ))));
    }

    let idp = state.km_store.insert_identity_provider(
        body.name,
        body.provider_type,
        body.enabled,
        body.config,
    )?;
    Ok((StatusCode::CREATED, Json(idp)))
}

pub async fn get_identity_provider(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<IdentityProvider>, ApiError> {
    require_super_admin(&claims, &state)?;
    let idp = state.km_store.get_identity_provider(IdpId(id))?;
    Ok(Json(idp))
}

pub async fn update_identity_provider(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    AppJson(body): AppJson<UpdateIdpRequest>,
) -> Result<Json<IdentityProvider>, ApiError> {
    require_super_admin(&claims, &state)?;
    let idp = state.km_store.update_identity_provider(
        IdpId(id),
        body.name,
        body.provider_type,
        body.enabled,
        body.config,
    )?;
    Ok(Json(idp))
}

pub async fn delete_identity_provider(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;
    state.km_store.delete_identity_provider(IdpId(id))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn test_idp_connection(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<TestConnectionResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    // Verify the IdP exists
    let idp = state.km_store.get_identity_provider(IdpId(id))?;
    // TODO: Implement actual connection testing per provider_type
    Ok(Json(TestConnectionResponse {
        success: false,
        message: format!(
            "Connection testing for '{}' providers is not yet implemented",
            idp.provider_type
        ),
    }))
}

// ── Provider config endpoints (super admin only) ────────────────────

fn config_to_response(p: &thairag_config::schema::ProvidersConfig) -> ProviderConfigResponse {
    let non_empty = |s: &str| -> Option<String> {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    ProviderConfigResponse {
        llm: LlmProviderInfo {
            kind: kind_str(&p.llm.kind),
            model: p.llm.model.clone(),
            base_url: non_empty(&p.llm.base_url),
            has_api_key: !p.llm.api_key.is_empty(),
            supports_vision: is_vision_model(&p.llm.kind, &p.llm.model),
            max_tokens: None,
            profile_id: p.llm.profile_id.clone(),
        },
        embedding: EmbeddingProviderInfo {
            kind: kind_str(&p.embedding.kind),
            model: p.embedding.model.clone(),
            dimension: p.embedding.dimension,
            base_url: non_empty(&p.embedding.base_url),
            has_api_key: !p.embedding.api_key.is_empty(),
        },
        vector_store: VectorStoreProviderInfo {
            kind: kind_str(&p.vector_store.kind),
            url: non_empty(&p.vector_store.url),
            collection: non_empty(&p.vector_store.collection),
            has_api_key: !p.vector_store.api_key.is_empty(),
            isolation: kind_str(&p.vector_store.isolation),
        },
        text_search: TextSearchProviderInfo {
            kind: kind_str(&p.text_search.kind),
            index_path: p.text_search.index_path.clone(),
        },
        reranker: RerankerProviderInfo {
            kind: kind_str(&p.reranker.kind),
            model: non_empty(&p.reranker.model),
            has_api_key: !p.reranker.api_key.is_empty(),
        },
    }
}

pub async fn get_provider_config(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<ProviderConfigResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    let p = &state.providers().providers_config;
    Ok(Json(config_to_response(p)))
}

// ── Update provider config DTO ──────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateProviderConfigRequest {
    pub llm: Option<UpdateLlmConfig>,
    pub embedding: Option<UpdateEmbeddingConfig>,
    pub vector_store: Option<UpdateVectorStoreConfig>,
    pub reranker: Option<UpdateRerankerConfig>,
}

#[derive(Deserialize)]
pub struct UpdateLlmConfig {
    pub kind: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub max_tokens: Option<u32>,
    /// Set a vault profile ID to resolve credentials from the vault.
    pub profile_id: Option<String>,
    /// When true, clear the current profile_id (set to None).
    pub clear_profile: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateEmbeddingConfig {
    pub kind: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateVectorStoreConfig {
    pub kind: Option<String>,
    pub url: Option<String>,
    pub collection: Option<String>,
    pub api_key: Option<String>,
    pub isolation: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateRerankerConfig {
    pub kind: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

pub async fn update_provider_config(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<UpdateProviderConfigRequest>,
) -> Result<Json<ProviderConfigResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Start from current config
    let mut pc = state.providers().providers_config.clone();

    // Apply partial updates
    if let Some(llm) = body.llm {
        let old_kind = pc.llm.kind.clone();
        if let Some(kind) = llm.kind {
            pc.llm.kind =
                parse_llm_kind(&kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
        }
        if let Some(model) = llm.model {
            pc.llm.model = model;
        }
        let explicit_base_url = llm.base_url.is_some();
        if let Some(base_url) = llm.base_url {
            pc.llm.base_url = base_url;
        }
        if let Some(api_key) = llm.api_key {
            pc.llm.api_key = api_key;
        }
        // When switching to a provider that uses its own default URL (Claude, OpenAI, Gemini),
        // clear base_url so it doesn't keep the old Ollama URL
        if pc.llm.kind != old_kind {
            use thairag_core::types::LlmKind;
            match pc.llm.kind {
                LlmKind::Ollama | LlmKind::OpenAiCompatible => {
                    // Keep base_url — user manages it
                }
                LlmKind::Claude | LlmKind::OpenAi | LlmKind::Gemini => {
                    if !explicit_base_url {
                        // Only clear if user didn't explicitly set a new base_url
                        pc.llm.base_url = String::new();
                    }
                }
            }
        }
    }
    if let Some(emb) = body.embedding {
        if let Some(kind) = emb.kind {
            pc.embedding.kind =
                parse_embedding_kind(&kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
        }
        if let Some(model) = emb.model {
            pc.embedding.model = model;
        }
        if let Some(dimension) = emb.dimension {
            pc.embedding.dimension = dimension;
        }
        if let Some(base_url) = emb.base_url {
            pc.embedding.base_url = base_url;
        }
        if let Some(api_key) = emb.api_key {
            pc.embedding.api_key = api_key;
        }
    }
    if let Some(vs) = body.vector_store {
        if let Some(kind) = vs.kind {
            pc.vector_store.kind = parse_vector_store_kind(&kind)
                .map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
        }
        if let Some(url) = vs.url {
            pc.vector_store.url = url;
        }
        if let Some(collection) = vs.collection {
            pc.vector_store.collection = collection;
        }
        if let Some(api_key) = vs.api_key {
            pc.vector_store.api_key = api_key;
        }
        if let Some(isolation) = vs.isolation {
            pc.vector_store.isolation = parse_vector_isolation(&isolation)
                .map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
        }
    }
    if let Some(rr) = body.reranker {
        if let Some(kind) = rr.kind {
            pc.reranker.kind =
                parse_reranker_kind(&kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
        }
        if let Some(model) = rr.model {
            pc.reranker.model = model;
        }
        if let Some(api_key) = rr.api_key {
            pc.reranker.api_key = api_key;
        }
    }

    // Detect embedding dimension or model change — clear stale vectors
    let old_embedding = &state.providers().providers_config.embedding;
    let embedding_changed = old_embedding.dimension != pc.embedding.dimension
        || old_embedding.model != pc.embedding.model
        || old_embedding.kind != pc.embedding.kind;

    // Validate the new config
    let mut validate_config = (*state.config).clone();
    validate_config.providers = pc.clone();
    validate_config
        .validate()
        .map_err(|e| ApiError(ThaiRagError::Validation(e)))?;

    // If embedding changed, auto-save a snapshot, clear old vectors, and update fingerprint
    if embedding_changed {
        let current_fp = get_embedding_fingerprint(&state);
        let new_fp = format!(
            "{:?}:{}:{}",
            pc.embedding.kind, pc.embedding.model, pc.embedding.dimension,
        );

        // Auto-save snapshot before destructive embedding change
        let settings: std::collections::HashMap<String, String> =
            state.km_store.list_all_settings().into_iter().collect();
        let auto_snap = ConfigSnapshot {
            id: Uuid::new_v4().to_string(),
            name: "Auto-save before config change".to_string(),
            description: format!(
                "Auto-saved before embedding model change from {}",
                current_fp
            ),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: claims.sub.clone(),
            embedding_fingerprint: current_fp.clone(),
            settings,
        };
        if let Ok(snap_json) = serde_json::to_string(&auto_snap) {
            state
                .km_store
                .set_setting(&format!("snapshot.{}", auto_snap.id), &snap_json);
            let mut ids: Vec<String> = state
                .km_store
                .get_setting("_snapshot_index")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            ids.push(auto_snap.id.clone());
            state
                .km_store
                .set_setting("_snapshot_index", &serde_json::to_string(&ids).unwrap());
            tracing::info!(
                snapshot_id = %auto_snap.id,
                "Auto-saved config snapshot before embedding change"
            );
        }

        tracing::warn!(
            old_model = %old_embedding.model,
            old_dim = old_embedding.dimension,
            new_model = %pc.embedding.model,
            new_dim = pc.embedding.dimension,
            "Embedding model changed — clearing vector store. Documents need re-processing."
        );
        let _ = state.providers().search_engine.delete_all_vectors().await;

        // Update embedding fingerprint
        state
            .km_store
            .set_setting("_embedding_fingerprint", &new_fp);
    }

    // If the LLM base_url changed, propagate to all per-agent LLM configs in the DB
    // that were using the old URL. This prevents stale URLs when the user changes the
    // Ollama port or host.
    let old_llm_url = &state.providers().providers_config.llm.base_url;
    if !old_llm_url.is_empty() && pc.llm.base_url != *old_llm_url {
        let agent_llm_keys = [
            "chat_pipeline.query_analyzer_llm",
            "chat_pipeline.query_rewriter_llm",
            "chat_pipeline.context_curator_llm",
            "chat_pipeline.response_generator_llm",
            "chat_pipeline.quality_guard_llm",
            "chat_pipeline.language_adapter_llm",
            "chat_pipeline.orchestrator_llm",
            "chat_pipeline.memory_llm",
            "chat_pipeline.tool_use_llm",
            "chat_pipeline.self_rag_llm",
            "chat_pipeline.graph_rag_llm",
            "chat_pipeline.map_reduce_llm",
            "chat_pipeline.ragas_llm",
            "chat_pipeline.compression_llm",
            "chat_pipeline.multimodal_llm",
            "chat_pipeline.raptor_llm",
            "chat_pipeline.colbert_llm",
            "chat_pipeline.personal_memory_llm",
            "chat_pipeline.crag_llm",
            "ai_preprocessing.analyzer_llm",
            "ai_preprocessing.converter_llm",
            "ai_preprocessing.quality_llm",
            "ai_preprocessing.chunker_llm",
            "ai_preprocessing.orchestrator_llm",
            "ai_preprocessing.enricher_llm",
        ];
        let mut updated_count = 0;
        for key in &agent_llm_keys {
            if let Some(val) = state.km_store.get_setting(key)
                && val.contains(old_llm_url.as_str())
            {
                let new_val = val.replace(old_llm_url.as_str(), &pc.llm.base_url);
                state.km_store.set_setting(key, &new_val);
                updated_count += 1;
            }
        }
        if updated_count > 0 {
            tracing::info!(
                old_url = %old_llm_url,
                new_url = %pc.llm.base_url,
                updated_count,
                "Propagated LLM base_url change to per-agent configs"
            );
        }
    }

    // Persist to DB
    let json = serde_json::to_string(&pc)
        .map_err(|e| ApiError(ThaiRagError::Internal(format!("Serialize failed: {e}"))))?;
    state.km_store.set_setting("provider_config", &json);

    // Hot-reload providers
    let eff_chat = crate::routes::settings::get_effective_chat_pipeline(&state);
    let bundle =
        state.build_provider_bundle(&pc, &state.config.search, &state.config.document, &eff_chat);
    state.reload_providers(bundle);

    tracing::info!("Provider config updated and hot-reloaded by super admin");

    Ok(Json(config_to_response(&pc)))
}

/// Fetch models for a given LLM kind/base_url/api_key combination.
async fn fetch_models_for_provider(
    kind: &thairag_core::types::LlmKind,
    base_url: &str,
    api_key: &str,
) -> ModelsResponse {
    use thairag_core::types::LlmKind;

    let kind_str = kind_str(kind);
    let client = reqwest::Client::new();

    match kind {
        LlmKind::Ollama => {
            let effective_url = if base_url.is_empty() {
                "http://localhost:11435"
            } else {
                base_url
            };
            let url = format!("{}/api/tags", effective_url.trim_end_matches('/'));
            match client
                .get(&url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let models = body["models"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .map(|m| AvailableModel {
                                        id: m["name"].as_str().unwrap_or("").to_string(),
                                        name: m["name"].as_str().unwrap_or("").to_string(),
                                        size: m["size"].as_u64(),
                                        modified_at: m["modified_at"]
                                            .as_str()
                                            .map(|s| s.to_string()),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        ModelsResponse {
                            provider: kind_str,
                            models,
                        }
                    } else {
                        ModelsResponse {
                            provider: kind_str,
                            models: vec![],
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to fetch Ollama models");
                    ModelsResponse {
                        provider: kind_str,
                        models: vec![],
                    }
                }
            }
        }
        LlmKind::Claude => {
            let models = vec![
                AvailableModel {
                    id: "claude-opus-4-20250514".into(),
                    name: "Claude Opus 4".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "claude-sonnet-4-20250514".into(),
                    name: "Claude Sonnet 4".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "claude-haiku-4-20250414".into(),
                    name: "Claude Haiku 4".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "claude-3-5-sonnet-20241022".into(),
                    name: "Claude 3.5 Sonnet".into(),
                    size: None,
                    modified_at: None,
                },
            ];
            ModelsResponse {
                provider: kind_str,
                models,
            }
        }
        LlmKind::OpenAi => {
            if api_key.is_empty() {
                return ModelsResponse {
                    provider: kind_str,
                    models: vec![],
                };
            }
            match client
                .get("https://api.openai.com/v1/models")
                .bearer_auth(api_key)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let models = body["data"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter(|m| {
                                        let id = m["id"].as_str().unwrap_or("");
                                        id.starts_with("gpt-")
                                            || id.starts_with("o1")
                                            || id.starts_with("o3")
                                            || id.starts_with("o4")
                                    })
                                    .map(|m| AvailableModel {
                                        id: m["id"].as_str().unwrap_or("").to_string(),
                                        name: m["id"].as_str().unwrap_or("").to_string(),
                                        size: None,
                                        modified_at: None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        ModelsResponse {
                            provider: kind_str,
                            models,
                        }
                    } else {
                        ModelsResponse {
                            provider: kind_str,
                            models: vec![],
                        }
                    }
                }
                Err(_) => ModelsResponse {
                    provider: kind_str,
                    models: vec![],
                },
            }
        }
        LlmKind::OpenAiCompatible => {
            if api_key.is_empty() || base_url.is_empty() {
                return ModelsResponse {
                    provider: kind_str,
                    models: vec![],
                };
            }
            let base = base_url.trim_end_matches('/');
            match client
                .get(format!("{base}/v1/models"))
                .bearer_auth(api_key)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let models = body["data"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .map(|m| AvailableModel {
                                        id: m["id"].as_str().unwrap_or("").to_string(),
                                        name: m["id"].as_str().unwrap_or("").to_string(),
                                        size: None,
                                        modified_at: None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        ModelsResponse {
                            provider: kind_str,
                            models,
                        }
                    } else {
                        ModelsResponse {
                            provider: kind_str,
                            models: vec![],
                        }
                    }
                }
                Err(_) => ModelsResponse {
                    provider: kind_str,
                    models: vec![],
                },
            }
        }
        LlmKind::Gemini => {
            if !api_key.is_empty() {
                // Try fetching from Gemini API
                match client
                    .get(format!(
                        "https://generativelanguage.googleapis.com/v1beta/models?key={api_key}"
                    ))
                    .timeout(std::time::Duration::from_secs(10))
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if let Ok(body) = resp.json::<serde_json::Value>().await {
                            let models: Vec<AvailableModel> = body["models"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter(|m| {
                                            // Only show generateContent-capable models
                                            m["supportedGenerationMethods"].as_array().is_some_and(
                                                |methods| {
                                                    methods.iter().any(|v| {
                                                        v.as_str() == Some("generateContent")
                                                    })
                                                },
                                            )
                                        })
                                        .map(|m| {
                                            let full_name = m["name"].as_str().unwrap_or("");
                                            let id = full_name
                                                .strip_prefix("models/")
                                                .unwrap_or(full_name);
                                            let display = m["displayName"].as_str().unwrap_or(id);
                                            AvailableModel {
                                                id: id.to_string(),
                                                name: display.to_string(),
                                                size: None,
                                                modified_at: None,
                                            }
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            if !models.is_empty() {
                                return ModelsResponse {
                                    provider: kind_str,
                                    models,
                                };
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to fetch Gemini models from API, using static list");
                    }
                }
            }
            // Fallback to static list
            let models = vec![
                AvailableModel {
                    id: "gemini-2.5-pro".into(),
                    name: "Gemini 2.5 Pro".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "gemini-2.5-flash".into(),
                    name: "Gemini 2.5 Flash".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "gemini-2.0-flash".into(),
                    name: "Gemini 2.0 Flash".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "gemini-1.5-pro".into(),
                    name: "Gemini 1.5 Pro".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "gemini-1.5-flash".into(),
                    name: "Gemini 1.5 Flash".into(),
                    size: None,
                    modified_at: None,
                },
            ];
            ModelsResponse {
                provider: kind_str,
                models,
            }
        }
    }
}

pub async fn list_available_models(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<ModelsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let pc = state.providers().providers_config;
    Ok(Json(
        fetch_models_for_provider(&pc.llm.kind, &pc.llm.base_url, &pc.llm.api_key).await,
    ))
}

#[derive(Deserialize)]
pub struct SyncModelsRequest {
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

/// POST /api/km/settings/providers/models/sync
/// Fetch models for a provider using the given credentials (without saving config).
/// Uses saved API key as fallback if none provided and kind matches current config.
pub async fn sync_models(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<SyncModelsRequest>,
) -> Result<Json<ModelsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let kind = parse_llm_kind(&req.kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;

    // Use saved credentials as fallback when the user hasn't entered new ones
    let pc = state.providers().providers_config;
    let api_key = if req.api_key.is_empty() && kind == pc.llm.kind {
        pc.llm.api_key.clone()
    } else {
        req.api_key
    };
    let base_url = if req.base_url.is_empty() && kind == pc.llm.kind {
        pc.llm.base_url.clone()
    } else {
        req.base_url
    };

    Ok(Json(
        fetch_models_for_provider(&kind, &base_url, &api_key).await,
    ))
}

// ── Embedding model sync ────────────────────────────────────────────

async fn fetch_models_for_embedding_provider(
    kind: &thairag_core::types::EmbeddingKind,
    base_url: &str,
    api_key: &str,
) -> ModelsResponse {
    use thairag_core::types::EmbeddingKind;

    let kind_str = kind_str(kind);
    let client = reqwest::Client::new();

    match kind {
        EmbeddingKind::Fastembed => {
            // Static list of popular fastembed models
            let models = vec![
                AvailableModel {
                    id: "BAAI/bge-small-en-v1.5".into(),
                    name: "BGE Small EN v1.5 (dim=384)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "BAAI/bge-base-en-v1.5".into(),
                    name: "BGE Base EN v1.5 (dim=768)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "BAAI/bge-large-en-v1.5".into(),
                    name: "BGE Large EN v1.5 (dim=1024)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "sentence-transformers/all-MiniLM-L6-v2".into(),
                    name: "All-MiniLM-L6-v2 (dim=384)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2".into(),
                    name: "Multilingual MiniLM L12 v2 (dim=384)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "jinaai/jina-embeddings-v2-small-en".into(),
                    name: "Jina Embeddings v2 Small EN (dim=512)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "jinaai/jina-embeddings-v2-base-en".into(),
                    name: "Jina Embeddings v2 Base EN (dim=768)".into(),
                    size: None,
                    modified_at: None,
                },
            ];
            ModelsResponse {
                provider: kind_str,
                models,
            }
        }
        EmbeddingKind::Ollama => {
            // Query Ollama /api/tags and filter for embedding models
            let effective_url = if base_url.is_empty() {
                "http://localhost:11435"
            } else {
                base_url
            };
            let url = format!("{}/api/tags", effective_url.trim_end_matches('/'));
            match client
                .get(&url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let models = body["models"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .map(|m| AvailableModel {
                                        id: m["name"].as_str().unwrap_or("").to_string(),
                                        name: m["name"].as_str().unwrap_or("").to_string(),
                                        size: m["size"].as_u64(),
                                        modified_at: m["modified_at"]
                                            .as_str()
                                            .map(|s| s.to_string()),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        ModelsResponse {
                            provider: kind_str,
                            models,
                        }
                    } else {
                        ModelsResponse {
                            provider: kind_str,
                            models: vec![],
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to fetch Ollama models for embedding");
                    ModelsResponse {
                        provider: kind_str,
                        models: vec![],
                    }
                }
            }
        }
        EmbeddingKind::OpenAi => {
            // Query OpenAI /v1/models and filter to embedding models
            if api_key.is_empty() {
                return ModelsResponse {
                    provider: kind_str,
                    models: vec![],
                };
            }
            let base = if base_url.is_empty() {
                "https://api.openai.com"
            } else {
                base_url.trim_end_matches('/')
            };
            match client
                .get(format!("{base}/v1/models"))
                .bearer_auth(api_key)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let models: Vec<AvailableModel> = body["data"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter(|m| {
                                        let id = m["id"].as_str().unwrap_or("");
                                        id.contains("embedding")
                                    })
                                    .map(|m| AvailableModel {
                                        id: m["id"].as_str().unwrap_or("").to_string(),
                                        name: m["id"].as_str().unwrap_or("").to_string(),
                                        size: None,
                                        modified_at: None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        ModelsResponse {
                            provider: kind_str,
                            models,
                        }
                    } else {
                        ModelsResponse {
                            provider: kind_str,
                            models: vec![],
                        }
                    }
                }
                Err(_) => ModelsResponse {
                    provider: kind_str,
                    models: vec![],
                },
            }
        }
        EmbeddingKind::Cohere => {
            // Static list of Cohere embedding models
            let models = vec![
                AvailableModel {
                    id: "embed-v4.0".into(),
                    name: "Embed v4.0 (dim=1024)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "embed-english-v3.0".into(),
                    name: "Embed English v3.0 (dim=1024)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "embed-multilingual-v3.0".into(),
                    name: "Embed Multilingual v3.0 (dim=1024)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "embed-english-light-v3.0".into(),
                    name: "Embed English Light v3.0 (dim=384)".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "embed-multilingual-light-v3.0".into(),
                    name: "Embed Multilingual Light v3.0 (dim=384)".into(),
                    size: None,
                    modified_at: None,
                },
            ];
            ModelsResponse {
                provider: kind_str,
                models,
            }
        }
    }
}

#[derive(Deserialize)]
pub struct SyncEmbeddingModelsRequest {
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

pub async fn sync_embedding_models(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<SyncEmbeddingModelsRequest>,
) -> Result<Json<ModelsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let kind =
        parse_embedding_kind(&req.kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;

    let pc = state.providers().providers_config;
    let api_key = if req.api_key.is_empty() && kind == pc.embedding.kind {
        pc.embedding.api_key.clone()
    } else {
        req.api_key
    };
    let base_url = if req.base_url.is_empty() && kind == pc.embedding.kind {
        pc.embedding.base_url.clone()
    } else {
        req.base_url
    };

    Ok(Json(
        fetch_models_for_embedding_provider(&kind, &base_url, &api_key).await,
    ))
}

// ── Reranker model sync ─────────────────────────────────────────────

fn reranker_static_models(kind: &thairag_core::types::RerankerKind) -> ModelsResponse {
    use thairag_core::types::RerankerKind;

    let kind_str = kind_str(kind);
    match kind {
        RerankerKind::Passthrough => ModelsResponse {
            provider: kind_str,
            models: vec![],
        },
        RerankerKind::Cohere => {
            let models = vec![
                AvailableModel {
                    id: "rerank-v3.5".into(),
                    name: "Rerank v3.5".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "rerank-english-v3.0".into(),
                    name: "Rerank English v3.0".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "rerank-multilingual-v3.0".into(),
                    name: "Rerank Multilingual v3.0".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "rerank-english-v2.0".into(),
                    name: "Rerank English v2.0".into(),
                    size: None,
                    modified_at: None,
                },
            ];
            ModelsResponse {
                provider: kind_str,
                models,
            }
        }
        RerankerKind::Jina => {
            let models = vec![
                AvailableModel {
                    id: "jina-reranker-v2-base-multilingual".into(),
                    name: "Jina Reranker v2 Base Multilingual".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "jina-reranker-v1-base-en".into(),
                    name: "Jina Reranker v1 Base EN".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "jina-reranker-v1-turbo-en".into(),
                    name: "Jina Reranker v1 Turbo EN".into(),
                    size: None,
                    modified_at: None,
                },
                AvailableModel {
                    id: "jina-reranker-v1-tiny-en".into(),
                    name: "Jina Reranker v1 Tiny EN".into(),
                    size: None,
                    modified_at: None,
                },
            ];
            ModelsResponse {
                provider: kind_str,
                models,
            }
        }
    }
}

#[derive(Deserialize)]
pub struct SyncRerankerModelsRequest {
    pub kind: thairag_core::types::RerankerKind,
}

pub async fn sync_reranker_models(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<SyncRerankerModelsRequest>,
) -> Result<Json<ModelsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    Ok(Json(reranker_static_models(&req.kind)))
}

// ── Document config endpoints ────────────────────────────────────────

#[derive(Serialize)]
pub struct DocumentConfigResponse {
    pub max_chunk_size: usize,
    pub chunk_overlap: usize,
    pub max_upload_size_mb: usize,
    pub ai_preprocessing: AiPreprocessingResponse,
}

#[derive(Serialize)]
pub struct AiPreprocessingResponse {
    pub enabled: bool,
    pub auto_params: bool,
    pub quality_threshold: f32,
    pub max_llm_input_chars: usize,
    pub agent_max_tokens: u32,
    pub min_ai_size_bytes: usize,
    /// Shared LLM for preprocessing. Null means using main chat LLM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmProviderInfo>,
    /// Per-agent LLM overrides. Null means using the shared preprocessing LLM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzer_llm: Option<LlmProviderInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub converter_llm: Option<LlmProviderInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_llm: Option<LlmProviderInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunker_llm: Option<LlmProviderInfo>,
    /// Retry-with-feedback settings.
    pub retry: AiRetryResponse,
    /// LLM-driven orchestration settings.
    pub orchestrator_enabled: bool,
    pub auto_orchestrator_budget: bool,
    pub max_orchestrator_calls: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator_llm: Option<LlmProviderInfo>,
    /// Chunk enrichment (context prefix, summary, keywords, HyDE).
    pub enricher_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enricher_llm: Option<LlmProviderInfo>,
}

#[derive(Serialize)]
pub struct AiRetryResponse {
    pub enabled: bool,
    pub converter_max_retries: u32,
    pub chunker_max_retries: u32,
    pub analyzer_max_retries: u32,
    pub analyzer_retry_below_confidence: f32,
}

#[derive(Deserialize)]
pub struct UpdateDocumentConfigRequest {
    pub max_chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
    pub max_upload_size_mb: Option<usize>,
    pub ai_preprocessing: Option<UpdateAiPreprocessing>,
}

#[derive(Deserialize)]
pub struct UpdateAiPreprocessing {
    pub enabled: Option<bool>,
    pub auto_params: Option<bool>,
    pub quality_threshold: Option<f32>,
    pub max_llm_input_chars: Option<usize>,
    pub agent_max_tokens: Option<u32>,
    pub min_ai_size_bytes: Option<usize>,
    /// Set a separate LLM for preprocessing.
    pub llm: Option<UpdateLlmConfig>,
    /// Set to true to remove the separate LLM and fall back to main chat LLM.
    #[serde(default)]
    pub remove_llm: Option<bool>,
    /// Per-agent LLM overrides. Each falls back to shared preprocessing LLM → main chat LLM.
    pub analyzer_llm: Option<UpdateLlmConfig>,
    pub converter_llm: Option<UpdateLlmConfig>,
    pub quality_llm: Option<UpdateLlmConfig>,
    pub chunker_llm: Option<UpdateLlmConfig>,
    /// Set to true to remove individual agent LLM overrides.
    #[serde(default)]
    pub remove_analyzer_llm: Option<bool>,
    #[serde(default)]
    pub remove_converter_llm: Option<bool>,
    #[serde(default)]
    pub remove_quality_llm: Option<bool>,
    #[serde(default)]
    pub remove_chunker_llm: Option<bool>,
    /// Retry-with-feedback settings.
    pub retry_enabled: Option<bool>,
    pub converter_max_retries: Option<u32>,
    pub chunker_max_retries: Option<u32>,
    pub analyzer_max_retries: Option<u32>,
    pub analyzer_retry_below_confidence: Option<f32>,
    /// Orchestrator settings.
    pub orchestrator_enabled: Option<bool>,
    pub auto_orchestrator_budget: Option<bool>,
    pub max_orchestrator_calls: Option<u32>,
    pub orchestrator_llm: Option<UpdateLlmConfig>,
    #[serde(default)]
    pub remove_orchestrator_llm: Option<bool>,
    /// Enricher settings.
    pub enricher_enabled: Option<bool>,
    pub enricher_llm: Option<UpdateLlmConfig>,
    #[serde(default)]
    pub remove_enricher_llm: Option<bool>,
}

fn llm_config_to_info(llm: &thairag_config::schema::LlmConfig) -> LlmProviderInfo {
    let non_empty = |s: &str| -> Option<String> {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    LlmProviderInfo {
        kind: kind_str(&llm.kind),
        model: llm.model.clone(),
        base_url: non_empty(&llm.base_url),
        has_api_key: !llm.api_key.is_empty(),
        supports_vision: is_vision_model(&llm.kind, &llm.model),
        max_tokens: llm.max_tokens,
        profile_id: llm.profile_id.clone(),
    }
}

/// Check if a model supports vision/image input based on provider + model name.
fn is_vision_model(kind: &thairag_core::types::LlmKind, model: &str) -> bool {
    use thairag_core::types::LlmKind;
    match kind {
        LlmKind::Claude => {
            model.contains("claude-3")
                || model.contains("claude-opus-4")
                || model.contains("claude-sonnet-4")
                || model.contains("claude-haiku-4")
        }
        LlmKind::OpenAi | LlmKind::OpenAiCompatible => {
            model.contains("gpt-4o")
                || model.contains("gpt-4.1")
                || model.contains("gpt-4-vision")
                || model.starts_with("o3")
                || model.starts_with("o4")
        }
        LlmKind::Gemini => model.contains("gemini-1.5") || model.contains("gemini-2"),
        LlmKind::Ollama => {
            let m = model.to_lowercase();
            m.contains("llava")
                || m.contains("llama3.2-vision")
                || m.contains("minicpm-v")
                || m.contains("bakllava")
                || m.contains("moondream")
                || m.contains("cogvlm")
                || m.contains("internvl")
                || m.contains("qwen2.5vl")
                || m.contains("qwen2-vl")
                || m.contains("qwenvl")
                || m.contains("gemma3")
        }
    }
}

/// Read effective preprocessing LLM config from KM store, falling back to file config.
fn get_effective_preprocessing_llm(state: &AppState) -> Option<thairag_config::schema::LlmConfig> {
    state
        .km_store
        .get_setting("ai_preprocessing.llm")
        .and_then(|json| serde_json::from_str(&json).ok())
        .or_else(|| state.config.document.ai_preprocessing.llm.clone())
}

/// Read effective per-agent LLM config from KM store, falling back to file config.
fn get_effective_agent_llm(
    state: &AppState,
    agent: &str,
) -> Option<thairag_config::schema::LlmConfig> {
    let key = format!("ai_preprocessing.{agent}_llm");
    state
        .km_store
        .get_setting(&key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .or_else(|| {
            let ai = &state.config.document.ai_preprocessing;
            match agent {
                "analyzer" => ai.analyzer_llm.clone(),
                "converter" => ai.converter_llm.clone(),
                "quality" => ai.quality_llm.clone(),
                "chunker" => ai.chunker_llm.clone(),
                "orchestrator" => ai.orchestrator_llm.clone(),
                "enricher" => ai.enricher_llm.clone(),
                _ => None,
            }
        })
}

/// Read all effective AI preprocessing settings from KM store + file config.
struct EffectiveAiConfig {
    enabled: bool,
    auto_params: bool,
    quality_threshold: f32,
    max_llm_input_chars: usize,
    agent_max_tokens: u32,
    min_ai_size_bytes: usize,
    retry_enabled: bool,
    converter_max_retries: u32,
    chunker_max_retries: u32,
    analyzer_max_retries: u32,
    analyzer_retry_below_confidence: f32,
    orchestrator_enabled: bool,
    auto_orchestrator_budget: bool,
    max_orchestrator_calls: u32,
    enricher_enabled: bool,
}

fn get_effective_ai_config(state: &AppState) -> EffectiveAiConfig {
    let ai = &state.config.document.ai_preprocessing;
    EffectiveAiConfig {
        enabled: state
            .km_store
            .get_setting("ai_preprocessing.enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.enabled),
        auto_params: state
            .km_store
            .get_setting("ai_preprocessing.auto_params")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.auto_params),
        quality_threshold: state
            .km_store
            .get_setting("ai_preprocessing.quality_threshold")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.quality_threshold),
        max_llm_input_chars: state
            .km_store
            .get_setting("ai_preprocessing.max_llm_input_chars")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.max_llm_input_chars),
        agent_max_tokens: state
            .km_store
            .get_setting("ai_preprocessing.agent_max_tokens")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.agent_max_tokens),
        min_ai_size_bytes: state
            .km_store
            .get_setting("ai_preprocessing.min_ai_size_bytes")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.min_ai_size_bytes),
        retry_enabled: state
            .km_store
            .get_setting("ai_preprocessing.retry.enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.retry.enabled),
        converter_max_retries: state
            .km_store
            .get_setting("ai_preprocessing.retry.converter_max_retries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.retry.converter_max_retries),
        chunker_max_retries: state
            .km_store
            .get_setting("ai_preprocessing.retry.chunker_max_retries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.retry.chunker_max_retries),
        analyzer_max_retries: state
            .km_store
            .get_setting("ai_preprocessing.retry.analyzer_max_retries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.retry.analyzer_max_retries),
        analyzer_retry_below_confidence: state
            .km_store
            .get_setting("ai_preprocessing.retry.analyzer_retry_below_confidence")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.retry.analyzer_retry_below_confidence),
        orchestrator_enabled: state
            .km_store
            .get_setting("ai_preprocessing.orchestrator_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.orchestrator_enabled),
        auto_orchestrator_budget: state
            .km_store
            .get_setting("ai_preprocessing.auto_orchestrator_budget")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.auto_orchestrator_budget),
        max_orchestrator_calls: state
            .km_store
            .get_setting("ai_preprocessing.max_orchestrator_calls")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.max_orchestrator_calls),
        enricher_enabled: state
            .km_store
            .get_setting("ai_preprocessing.enricher_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(ai.enricher_enabled),
    }
}

fn build_ai_preprocessing_response(state: &AppState) -> AiPreprocessingResponse {
    let effective_ai = get_effective_ai_config(state);
    AiPreprocessingResponse {
        enabled: effective_ai.enabled,
        auto_params: effective_ai.auto_params,
        quality_threshold: effective_ai.quality_threshold,
        max_llm_input_chars: effective_ai.max_llm_input_chars,
        agent_max_tokens: effective_ai.agent_max_tokens,
        min_ai_size_bytes: effective_ai.min_ai_size_bytes,
        llm: get_effective_preprocessing_llm(state)
            .as_ref()
            .map(llm_config_to_info),
        analyzer_llm: get_effective_agent_llm(state, "analyzer")
            .as_ref()
            .map(llm_config_to_info),
        converter_llm: get_effective_agent_llm(state, "converter")
            .as_ref()
            .map(llm_config_to_info),
        quality_llm: get_effective_agent_llm(state, "quality")
            .as_ref()
            .map(llm_config_to_info),
        chunker_llm: get_effective_agent_llm(state, "chunker")
            .as_ref()
            .map(llm_config_to_info),
        retry: AiRetryResponse {
            enabled: effective_ai.retry_enabled,
            converter_max_retries: effective_ai.converter_max_retries,
            chunker_max_retries: effective_ai.chunker_max_retries,
            analyzer_max_retries: effective_ai.analyzer_max_retries,
            analyzer_retry_below_confidence: effective_ai.analyzer_retry_below_confidence,
        },
        orchestrator_enabled: effective_ai.orchestrator_enabled,
        auto_orchestrator_budget: effective_ai.auto_orchestrator_budget,
        max_orchestrator_calls: effective_ai.max_orchestrator_calls,
        orchestrator_llm: get_effective_agent_llm(state, "orchestrator")
            .as_ref()
            .map(llm_config_to_info),
        enricher_enabled: effective_ai.enricher_enabled,
        enricher_llm: get_effective_agent_llm(state, "enricher")
            .as_ref()
            .map(llm_config_to_info),
    }
}

pub async fn get_document_config(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<DocumentConfigResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    let doc = &state.config.document;
    Ok(Json(DocumentConfigResponse {
        max_chunk_size: state
            .km_store
            .get_setting("document.max_chunk_size")
            .and_then(|v| v.parse().ok())
            .unwrap_or(doc.max_chunk_size),
        chunk_overlap: state
            .km_store
            .get_setting("document.chunk_overlap")
            .and_then(|v| v.parse().ok())
            .unwrap_or(doc.chunk_overlap),
        max_upload_size_mb: state
            .km_store
            .get_setting("document.max_upload_size_mb")
            .and_then(|v| v.parse().ok())
            .unwrap_or(doc.max_upload_size_mb),
        ai_preprocessing: build_ai_preprocessing_response(&state),
    }))
}

pub async fn update_document_config(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<UpdateDocumentConfigRequest>,
) -> Result<Json<DocumentConfigResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Persist pipeline settings
    if let Some(v) = req.max_chunk_size {
        if !(64..=100_000).contains(&v) {
            return Err(ApiError(ThaiRagError::Validation(
                "max_chunk_size must be between 64 and 100000".into(),
            )));
        }
        state
            .km_store
            .set_setting("document.max_chunk_size", &v.to_string());
    }
    if let Some(v) = req.chunk_overlap {
        if v > 10_000 {
            return Err(ApiError(ThaiRagError::Validation(
                "chunk_overlap must be at most 10000".into(),
            )));
        }
        state
            .km_store
            .set_setting("document.chunk_overlap", &v.to_string());
    }
    if let Some(v) = req.max_upload_size_mb {
        if !(1..=1024).contains(&v) {
            return Err(ApiError(ThaiRagError::Validation(
                "max_upload_size_mb must be between 1 and 1024".into(),
            )));
        }
        state
            .km_store
            .set_setting("document.max_upload_size_mb", &v.to_string());
    }

    if let Some(ai_update) = &req.ai_preprocessing {
        // Persist scalar AI settings
        if let Some(enabled) = ai_update.enabled {
            state
                .km_store
                .set_setting("ai_preprocessing.enabled", &enabled.to_string());
        }
        if let Some(auto_params) = ai_update.auto_params {
            state
                .km_store
                .set_setting("ai_preprocessing.auto_params", &auto_params.to_string());
        }
        if let Some(threshold) = ai_update.quality_threshold {
            state
                .km_store
                .set_setting("ai_preprocessing.quality_threshold", &threshold.to_string());
        }
        if let Some(chars) = ai_update.max_llm_input_chars {
            state
                .km_store
                .set_setting("ai_preprocessing.max_llm_input_chars", &chars.to_string());
        }
        if let Some(tokens) = ai_update.agent_max_tokens {
            state
                .km_store
                .set_setting("ai_preprocessing.agent_max_tokens", &tokens.to_string());
        }
        if let Some(size) = ai_update.min_ai_size_bytes {
            state
                .km_store
                .set_setting("ai_preprocessing.min_ai_size_bytes", &size.to_string());
        }

        // Persist retry-with-feedback settings
        if let Some(v) = ai_update.retry_enabled {
            state
                .km_store
                .set_setting("ai_preprocessing.retry.enabled", &v.to_string());
        }
        if let Some(v) = ai_update.converter_max_retries {
            state.km_store.set_setting(
                "ai_preprocessing.retry.converter_max_retries",
                &v.to_string(),
            );
        }
        if let Some(v) = ai_update.chunker_max_retries {
            state
                .km_store
                .set_setting("ai_preprocessing.retry.chunker_max_retries", &v.to_string());
        }
        if let Some(v) = ai_update.analyzer_max_retries {
            state.km_store.set_setting(
                "ai_preprocessing.retry.analyzer_max_retries",
                &v.to_string(),
            );
        }
        if let Some(v) = ai_update.analyzer_retry_below_confidence {
            state.km_store.set_setting(
                "ai_preprocessing.retry.analyzer_retry_below_confidence",
                &v.to_string(),
            );
        }

        // Persist orchestrator settings
        if let Some(v) = ai_update.orchestrator_enabled {
            state
                .km_store
                .set_setting("ai_preprocessing.orchestrator_enabled", &v.to_string());
        }
        if let Some(v) = ai_update.auto_orchestrator_budget {
            state
                .km_store
                .set_setting("ai_preprocessing.auto_orchestrator_budget", &v.to_string());
        }
        if let Some(v) = ai_update.max_orchestrator_calls {
            state
                .km_store
                .set_setting("ai_preprocessing.max_orchestrator_calls", &v.to_string());
        }

        // Helper: persist an LLM config update to a given KM store key
        fn persist_llm_update(
            state: &AppState,
            key: &str,
            llm_update: &UpdateLlmConfig,
            current: Option<thairag_config::schema::LlmConfig>,
        ) -> Result<(), ApiError> {
            let mut llm_config = current.unwrap_or_else(|| state.config.providers.llm.clone());
            if let Some(kind) = &llm_update.kind {
                llm_config.kind =
                    parse_llm_kind(kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
            }
            if let Some(model) = &llm_update.model {
                llm_config.model = model.clone();
            }
            if let Some(base_url) = &llm_update.base_url {
                llm_config.base_url = base_url.clone();
            }
            if let Some(api_key) = &llm_update.api_key
                && !api_key.is_empty()
            {
                llm_config.api_key = api_key.clone();
            }
            if let Some(max_tokens) = llm_update.max_tokens {
                llm_config.max_tokens = Some(max_tokens);
            }
            let json = serde_json::to_string(&llm_config).map_err(|e| {
                ApiError(ThaiRagError::Internal(format!("Serialize LLM config: {e}")))
            })?;
            state.km_store.set_setting(key, &json);
            Ok(())
        }

        // Persist shared preprocessing LLM config
        if let Some(llm_update) = &ai_update.llm {
            let current = get_effective_preprocessing_llm(&state);
            persist_llm_update(&state, "ai_preprocessing.llm", llm_update, current)?;
        }

        // Persist per-agent LLM configs
        if let Some(ref u) = ai_update.analyzer_llm {
            persist_llm_update(
                &state,
                "ai_preprocessing.analyzer_llm",
                u,
                get_effective_agent_llm(&state, "analyzer"),
            )?;
        }
        if let Some(ref u) = ai_update.converter_llm {
            persist_llm_update(
                &state,
                "ai_preprocessing.converter_llm",
                u,
                get_effective_agent_llm(&state, "converter"),
            )?;
        }
        if let Some(ref u) = ai_update.quality_llm {
            persist_llm_update(
                &state,
                "ai_preprocessing.quality_llm",
                u,
                get_effective_agent_llm(&state, "quality"),
            )?;
        }
        if let Some(ref u) = ai_update.chunker_llm {
            persist_llm_update(
                &state,
                "ai_preprocessing.chunker_llm",
                u,
                get_effective_agent_llm(&state, "chunker"),
            )?;
        }
        if let Some(ref u) = ai_update.orchestrator_llm {
            persist_llm_update(
                &state,
                "ai_preprocessing.orchestrator_llm",
                u,
                get_effective_agent_llm(&state, "orchestrator"),
            )?;
        }

        // Persist enricher settings
        if let Some(v) = ai_update.enricher_enabled {
            state
                .km_store
                .set_setting("ai_preprocessing.enricher_enabled", &v.to_string());
        }
        if let Some(ref u) = ai_update.enricher_llm {
            persist_llm_update(
                &state,
                "ai_preprocessing.enricher_llm",
                u,
                get_effective_agent_llm(&state, "enricher"),
            )?;
        }
    }

    // Handle explicit removal of LLM overrides
    if let Some(ai_update) = &req.ai_preprocessing {
        if ai_update.llm.is_none() && ai_update.remove_llm.unwrap_or(false) {
            state.km_store.delete_setting("ai_preprocessing.llm");
        }
        if ai_update.analyzer_llm.is_none() && ai_update.remove_analyzer_llm.unwrap_or(false) {
            state
                .km_store
                .delete_setting("ai_preprocessing.analyzer_llm");
        }
        if ai_update.converter_llm.is_none() && ai_update.remove_converter_llm.unwrap_or(false) {
            state
                .km_store
                .delete_setting("ai_preprocessing.converter_llm");
        }
        if ai_update.quality_llm.is_none() && ai_update.remove_quality_llm.unwrap_or(false) {
            state
                .km_store
                .delete_setting("ai_preprocessing.quality_llm");
        }
        if ai_update.chunker_llm.is_none() && ai_update.remove_chunker_llm.unwrap_or(false) {
            state
                .km_store
                .delete_setting("ai_preprocessing.chunker_llm");
        }
        if ai_update.orchestrator_llm.is_none()
            && ai_update.remove_orchestrator_llm.unwrap_or(false)
        {
            state
                .km_store
                .delete_setting("ai_preprocessing.orchestrator_llm");
        }
        if ai_update.enricher_llm.is_none() && ai_update.remove_enricher_llm.unwrap_or(false) {
            state
                .km_store
                .delete_setting("ai_preprocessing.enricher_llm");
        }
    }

    // Read back effective config
    let doc = &state.config.document;
    let effective_ai = get_effective_ai_config(&state);

    // Effective pipeline settings (overrides from KM store, fallback to config)
    let eff_max_chunk_size = state
        .km_store
        .get_setting("document.max_chunk_size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(doc.max_chunk_size);
    let eff_chunk_overlap = state
        .km_store
        .get_setting("document.chunk_overlap")
        .and_then(|v| v.parse().ok())
        .unwrap_or(doc.chunk_overlap);
    let eff_max_upload_size_mb = state
        .km_store
        .get_setting("document.max_upload_size_mb")
        .and_then(|v| v.parse().ok())
        .unwrap_or(doc.max_upload_size_mb);

    // Hot-reload the document pipeline with updated config
    let mut effective_doc = doc.clone();
    effective_doc.max_chunk_size = eff_max_chunk_size;
    effective_doc.chunk_overlap = eff_chunk_overlap;
    effective_doc.max_upload_size_mb = eff_max_upload_size_mb;
    effective_doc.ai_preprocessing.enabled = effective_ai.enabled;
    effective_doc.ai_preprocessing.auto_params = effective_ai.auto_params;
    effective_doc.ai_preprocessing.quality_threshold = effective_ai.quality_threshold;
    effective_doc.ai_preprocessing.max_llm_input_chars = effective_ai.max_llm_input_chars;
    effective_doc.ai_preprocessing.agent_max_tokens = effective_ai.agent_max_tokens;
    effective_doc.ai_preprocessing.min_ai_size_bytes = effective_ai.min_ai_size_bytes;
    effective_doc.ai_preprocessing.llm = get_effective_preprocessing_llm(&state);
    effective_doc.ai_preprocessing.analyzer_llm = get_effective_agent_llm(&state, "analyzer");
    effective_doc.ai_preprocessing.converter_llm = get_effective_agent_llm(&state, "converter");
    effective_doc.ai_preprocessing.quality_llm = get_effective_agent_llm(&state, "quality");
    effective_doc.ai_preprocessing.chunker_llm = get_effective_agent_llm(&state, "chunker");
    effective_doc.ai_preprocessing.retry.enabled = effective_ai.retry_enabled;
    effective_doc.ai_preprocessing.retry.converter_max_retries = effective_ai.converter_max_retries;
    effective_doc.ai_preprocessing.retry.chunker_max_retries = effective_ai.chunker_max_retries;
    effective_doc.ai_preprocessing.retry.analyzer_max_retries = effective_ai.analyzer_max_retries;
    effective_doc
        .ai_preprocessing
        .retry
        .analyzer_retry_below_confidence = effective_ai.analyzer_retry_below_confidence;
    effective_doc.ai_preprocessing.orchestrator_enabled = effective_ai.orchestrator_enabled;
    effective_doc.ai_preprocessing.auto_orchestrator_budget = effective_ai.auto_orchestrator_budget;
    effective_doc.ai_preprocessing.max_orchestrator_calls = effective_ai.max_orchestrator_calls;
    effective_doc.ai_preprocessing.orchestrator_llm =
        get_effective_agent_llm(&state, "orchestrator");
    effective_doc.ai_preprocessing.enricher_enabled = effective_ai.enricher_enabled;
    effective_doc.ai_preprocessing.enricher_llm = get_effective_agent_llm(&state, "enricher");

    // Try hot-reload; don't fail the save if provider creation has issues
    let eff_chat = get_effective_chat_pipeline(&state);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.build_provider_bundle(
            &state.providers().providers_config,
            &state.config.search,
            &effective_doc,
            &eff_chat,
        )
    })) {
        Ok(bundle) => {
            state.reload_providers(bundle);
            tracing::info!("Document processing config updated and hot-reloaded by super admin");
        }
        Err(e) => {
            tracing::warn!(
                "Hot-reload failed after document config save: {:?}. Config is saved but will take effect on restart.",
                e
            );
        }
    }

    Ok(Json(DocumentConfigResponse {
        max_chunk_size: eff_max_chunk_size,
        chunk_overlap: eff_chunk_overlap,
        max_upload_size_mb: eff_max_upload_size_mb,
        ai_preprocessing: build_ai_preprocessing_response(&state),
    }))
}

// ── Chat Pipeline Config ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct ChatPipelineConfigResponse {
    pub enabled: bool,
    pub llm_mode: String,
    pub llm: Option<LlmProviderInfo>,
    pub query_analyzer_enabled: bool,
    pub query_analyzer_llm: Option<LlmProviderInfo>,
    pub query_rewriter_enabled: bool,
    pub query_rewriter_llm: Option<LlmProviderInfo>,
    pub context_curator_enabled: bool,
    pub context_curator_llm: Option<LlmProviderInfo>,
    pub response_generator_llm: Option<LlmProviderInfo>,
    pub quality_guard_enabled: bool,
    pub quality_guard_llm: Option<LlmProviderInfo>,
    pub quality_guard_max_retries: u32,
    pub quality_guard_threshold: f32,
    pub language_adapter_enabled: bool,
    pub language_adapter_llm: Option<LlmProviderInfo>,
    pub orchestrator_enabled: bool,
    pub max_orchestrator_calls: u32,
    pub orchestrator_llm: Option<LlmProviderInfo>,
    pub max_context_tokens: usize,
    pub agent_max_tokens: u32,
    pub request_timeout_secs: u64,
    pub ollama_keep_alive: String,
    // Feature: Conversation Memory
    pub conversation_memory_enabled: bool,
    pub memory_max_summaries: usize,
    pub memory_summary_max_tokens: u32,
    pub memory_llm: Option<LlmProviderInfo>,
    // Feature: Multi-turn Retrieval Refinement
    pub retrieval_refinement_enabled: bool,
    pub refinement_min_relevance: f32,
    pub refinement_max_retries: u32,
    // Feature: Agentic Tool Use
    pub tool_use_enabled: bool,
    pub tool_use_max_calls: u32,
    pub tool_use_llm: Option<LlmProviderInfo>,
    // Feature: Adaptive Quality Thresholds
    pub adaptive_threshold_enabled: bool,
    pub feedback_decay_days: u32,
    pub adaptive_min_samples: u32,
    // Self-RAG
    pub self_rag_enabled: bool,
    pub self_rag_threshold: f32,
    pub self_rag_llm: Option<LlmProviderInfo>,
    // Graph RAG
    pub graph_rag_enabled: bool,
    pub graph_rag_max_entities: u32,
    pub graph_rag_max_depth: u32,
    pub graph_rag_llm: Option<LlmProviderInfo>,
    // CRAG
    pub crag_enabled: bool,
    pub crag_relevance_threshold: f32,
    pub crag_web_search_url: String,
    pub crag_max_web_results: u32,
    // Speculative RAG
    pub speculative_rag_enabled: bool,
    pub speculative_candidates: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speculative_rag_llm: Option<LlmProviderInfo>,
    // Map-Reduce RAG
    pub map_reduce_enabled: bool,
    pub map_reduce_max_chunks: usize,
    pub map_reduce_llm: Option<LlmProviderInfo>,
    // RAGAS
    pub ragas_enabled: bool,
    pub ragas_sample_rate: f32,
    pub ragas_llm: Option<LlmProviderInfo>,
    // Contextual Compression
    pub compression_enabled: bool,
    pub compression_target_ratio: f32,
    pub compression_llm: Option<LlmProviderInfo>,
    // Multi-modal RAG
    pub multimodal_enabled: bool,
    pub multimodal_max_images: u32,
    pub multimodal_llm: Option<LlmProviderInfo>,
    // RAPTOR
    pub raptor_enabled: bool,
    pub raptor_max_depth: u32,
    pub raptor_group_size: usize,
    pub raptor_llm: Option<LlmProviderInfo>,
    // ColBERT
    pub colbert_enabled: bool,
    pub colbert_top_n: usize,
    pub colbert_llm: Option<LlmProviderInfo>,
    // Active Learning
    pub active_learning_enabled: bool,
    pub active_learning_min_interactions: u32,
    pub active_learning_max_low_confidence: usize,
    // Context Compaction
    pub context_compaction_enabled: bool,
    pub model_context_window: usize,
    pub compaction_threshold: f32,
    pub compaction_keep_recent: usize,
    // Personal Memory
    pub personal_memory_enabled: bool,
    pub personal_memory_top_k: usize,
    pub personal_memory_max_per_user: usize,
    pub personal_memory_decay_factor: f32,
    pub personal_memory_min_relevance: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personal_memory_llm: Option<LlmProviderInfo>,
    // Live Source Retrieval
    pub live_retrieval_enabled: bool,
    pub live_retrieval_timeout_secs: u64,
    pub live_retrieval_max_connectors: u32,
    pub live_retrieval_max_content_chars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_retrieval_llm: Option<LlmProviderInfo>,
}

#[derive(Deserialize)]
pub struct UpdateChatPipelineRequest {
    pub enabled: Option<bool>,
    pub llm_mode: Option<String>,
    pub llm: Option<UpdateLlmConfig>,
    pub remove_llm: Option<bool>,
    pub query_analyzer_enabled: Option<bool>,
    pub query_analyzer_llm: Option<UpdateLlmConfig>,
    pub remove_query_analyzer_llm: Option<bool>,
    pub query_rewriter_enabled: Option<bool>,
    pub query_rewriter_llm: Option<UpdateLlmConfig>,
    pub remove_query_rewriter_llm: Option<bool>,
    pub context_curator_enabled: Option<bool>,
    pub context_curator_llm: Option<UpdateLlmConfig>,
    pub remove_context_curator_llm: Option<bool>,
    pub response_generator_llm: Option<UpdateLlmConfig>,
    pub remove_response_generator_llm: Option<bool>,
    pub quality_guard_enabled: Option<bool>,
    pub quality_guard_llm: Option<UpdateLlmConfig>,
    pub remove_quality_guard_llm: Option<bool>,
    pub quality_guard_max_retries: Option<u32>,
    pub quality_guard_threshold: Option<f32>,
    pub language_adapter_enabled: Option<bool>,
    pub language_adapter_llm: Option<UpdateLlmConfig>,
    pub remove_language_adapter_llm: Option<bool>,
    pub orchestrator_enabled: Option<bool>,
    pub max_orchestrator_calls: Option<u32>,
    pub orchestrator_llm: Option<UpdateLlmConfig>,
    pub remove_orchestrator_llm: Option<bool>,
    pub max_context_tokens: Option<usize>,
    pub agent_max_tokens: Option<u32>,
    pub request_timeout_secs: Option<u64>,
    pub ollama_keep_alive: Option<String>,
    // Feature: Conversation Memory
    pub conversation_memory_enabled: Option<bool>,
    pub memory_max_summaries: Option<usize>,
    pub memory_summary_max_tokens: Option<u32>,
    pub memory_llm: Option<UpdateLlmConfig>,
    pub remove_memory_llm: Option<bool>,
    // Feature: Multi-turn Retrieval Refinement
    pub retrieval_refinement_enabled: Option<bool>,
    pub refinement_min_relevance: Option<f32>,
    pub refinement_max_retries: Option<u32>,
    // Feature: Agentic Tool Use
    pub tool_use_enabled: Option<bool>,
    pub tool_use_max_calls: Option<u32>,
    pub tool_use_llm: Option<UpdateLlmConfig>,
    pub remove_tool_use_llm: Option<bool>,
    // Feature: Adaptive Quality Thresholds
    pub adaptive_threshold_enabled: Option<bool>,
    pub feedback_decay_days: Option<u32>,
    pub adaptive_min_samples: Option<u32>,
    // Self-RAG
    pub self_rag_enabled: Option<bool>,
    pub self_rag_threshold: Option<f32>,
    pub self_rag_llm: Option<UpdateLlmConfig>,
    pub remove_self_rag_llm: Option<bool>,
    // Graph RAG
    pub graph_rag_enabled: Option<bool>,
    pub graph_rag_max_entities: Option<u32>,
    pub graph_rag_max_depth: Option<u32>,
    pub graph_rag_llm: Option<UpdateLlmConfig>,
    pub remove_graph_rag_llm: Option<bool>,
    // CRAG
    pub crag_enabled: Option<bool>,
    pub crag_relevance_threshold: Option<f32>,
    pub crag_web_search_url: Option<String>,
    pub crag_max_web_results: Option<u32>,
    // Speculative RAG
    pub speculative_rag_enabled: Option<bool>,
    pub speculative_candidates: Option<u32>,
    pub speculative_rag_llm: Option<UpdateLlmConfig>,
    pub remove_speculative_rag_llm: Option<bool>,
    // Map-Reduce RAG
    pub map_reduce_enabled: Option<bool>,
    pub map_reduce_max_chunks: Option<usize>,
    pub map_reduce_llm: Option<UpdateLlmConfig>,
    pub remove_map_reduce_llm: Option<bool>,
    // RAGAS
    pub ragas_enabled: Option<bool>,
    pub ragas_sample_rate: Option<f32>,
    pub ragas_llm: Option<UpdateLlmConfig>,
    pub remove_ragas_llm: Option<bool>,
    // Contextual Compression
    pub compression_enabled: Option<bool>,
    pub compression_target_ratio: Option<f32>,
    pub compression_llm: Option<UpdateLlmConfig>,
    pub remove_compression_llm: Option<bool>,
    // Multi-modal RAG
    pub multimodal_enabled: Option<bool>,
    pub multimodal_max_images: Option<u32>,
    pub multimodal_llm: Option<UpdateLlmConfig>,
    pub remove_multimodal_llm: Option<bool>,
    // RAPTOR
    pub raptor_enabled: Option<bool>,
    pub raptor_max_depth: Option<u32>,
    pub raptor_group_size: Option<usize>,
    pub raptor_llm: Option<UpdateLlmConfig>,
    pub remove_raptor_llm: Option<bool>,
    // ColBERT
    pub colbert_enabled: Option<bool>,
    pub colbert_top_n: Option<usize>,
    pub colbert_llm: Option<UpdateLlmConfig>,
    pub remove_colbert_llm: Option<bool>,
    // Active Learning
    pub active_learning_enabled: Option<bool>,
    pub active_learning_min_interactions: Option<u32>,
    pub active_learning_max_low_confidence: Option<usize>,
    // Context Compaction
    pub context_compaction_enabled: Option<bool>,
    pub model_context_window: Option<usize>,
    pub compaction_threshold: Option<f32>,
    pub compaction_keep_recent: Option<usize>,
    // Personal Memory
    pub personal_memory_enabled: Option<bool>,
    pub personal_memory_top_k: Option<usize>,
    pub personal_memory_max_per_user: Option<usize>,
    pub personal_memory_decay_factor: Option<f32>,
    pub personal_memory_min_relevance: Option<f32>,
    pub personal_memory_llm: Option<UpdateLlmConfig>,
    pub remove_personal_memory_llm: Option<bool>,
    // Live Source Retrieval
    pub live_retrieval_enabled: Option<bool>,
    pub live_retrieval_timeout_secs: Option<u64>,
    pub live_retrieval_max_connectors: Option<u32>,
    pub live_retrieval_max_content_chars: Option<usize>,
    pub live_retrieval_llm: Option<UpdateLlmConfig>,
    pub remove_live_retrieval_llm: Option<bool>,
}

pub fn get_effective_chat_pipeline(state: &AppState) -> thairag_config::schema::ChatPipelineConfig {
    get_effective_chat_pipeline_with_store(&state.config, &*state.km_store)
}

/// Get effective chat pipeline config for a specific scope.
/// Uses batch resolution (at most 4 DB queries) instead of individual reads.
pub fn get_effective_chat_pipeline_scoped(
    config: &thairag_config::AppConfig,
    store: &dyn crate::store::KmStoreTrait,
    scope: &SettingsScope,
) -> thairag_config::schema::ChatPipelineConfig {
    if matches!(scope, SettingsScope::Global) {
        return get_effective_chat_pipeline_with_store(config, store);
    }
    let settings = crate::store::resolve_all_settings(store, scope);
    get_effective_chat_pipeline_from_map(config, &settings)
}

/// Build `ChatPipelineConfig` from a pre-resolved settings map (used by scoped resolution).
fn get_effective_chat_pipeline_from_map(
    config: &thairag_config::AppConfig,
    settings: &std::collections::HashMap<String, String>,
) -> thairag_config::schema::ChatPipelineConfig {
    let cp = &config.chat_pipeline;
    let s = |key: &str| settings.get(key).cloned();

    // Reuse the exact same field resolution logic as get_effective_chat_pipeline_with_store
    get_effective_chat_pipeline_from_getter(cp, s)
}

/// Same as `get_effective_chat_pipeline` but takes raw parts — usable before AppState exists.
pub fn get_effective_chat_pipeline_with_store(
    config: &thairag_config::AppConfig,
    store: &dyn crate::store::KmStoreTrait,
) -> thairag_config::schema::ChatPipelineConfig {
    let cp = &config.chat_pipeline;
    let s = |key: &str| store.get_setting(key);
    get_effective_chat_pipeline_from_getter(cp, s)
}

/// Internal: builds ChatPipelineConfig from a getter function.
fn get_effective_chat_pipeline_from_getter<F>(
    cp: &thairag_config::schema::ChatPipelineConfig,
    s: F,
) -> thairag_config::schema::ChatPipelineConfig
where
    F: Fn(&str) -> Option<String>,
{
    thairag_config::schema::ChatPipelineConfig {
        enabled: s("chat_pipeline.enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.enabled),
        llm: s("chat_pipeline.llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.llm.clone()),
        query_analyzer_enabled: s("chat_pipeline.query_analyzer_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.query_analyzer_enabled),
        query_analyzer_llm: s("chat_pipeline.query_analyzer_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.query_analyzer_llm.clone()),
        query_rewriter_enabled: s("chat_pipeline.query_rewriter_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.query_rewriter_enabled),
        query_rewriter_llm: s("chat_pipeline.query_rewriter_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.query_rewriter_llm.clone()),
        context_curator_enabled: s("chat_pipeline.context_curator_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.context_curator_enabled),
        context_curator_llm: s("chat_pipeline.context_curator_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.context_curator_llm.clone()),
        response_generator_llm: s("chat_pipeline.response_generator_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.response_generator_llm.clone()),
        quality_guard_enabled: s("chat_pipeline.quality_guard_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.quality_guard_enabled),
        quality_guard_llm: s("chat_pipeline.quality_guard_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.quality_guard_llm.clone()),
        quality_guard_max_retries: s("chat_pipeline.quality_guard_max_retries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.quality_guard_max_retries),
        quality_guard_threshold: s("chat_pipeline.quality_guard_threshold")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.quality_guard_threshold),
        language_adapter_enabled: s("chat_pipeline.language_adapter_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.language_adapter_enabled),
        language_adapter_llm: s("chat_pipeline.language_adapter_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.language_adapter_llm.clone()),
        orchestrator_enabled: s("chat_pipeline.orchestrator_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.orchestrator_enabled),
        max_orchestrator_calls: s("chat_pipeline.max_orchestrator_calls")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.max_orchestrator_calls),
        orchestrator_llm: s("chat_pipeline.orchestrator_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.orchestrator_llm.clone()),
        max_context_tokens: s("chat_pipeline.max_context_tokens")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.max_context_tokens),
        agent_max_tokens: s("chat_pipeline.agent_max_tokens")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.agent_max_tokens),
        // Feature: Conversation Memory
        conversation_memory_enabled: s("chat_pipeline.conversation_memory_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.conversation_memory_enabled),
        memory_max_summaries: s("chat_pipeline.memory_max_summaries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.memory_max_summaries),
        memory_summary_max_tokens: s("chat_pipeline.memory_summary_max_tokens")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.memory_summary_max_tokens),
        memory_llm: s("chat_pipeline.memory_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.memory_llm.clone()),
        // Feature: Multi-turn Retrieval Refinement
        retrieval_refinement_enabled: s("chat_pipeline.retrieval_refinement_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.retrieval_refinement_enabled),
        refinement_min_relevance: s("chat_pipeline.refinement_min_relevance")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.refinement_min_relevance),
        refinement_max_retries: s("chat_pipeline.refinement_max_retries")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.refinement_max_retries),
        // Feature: Agentic Tool Use
        tool_use_enabled: s("chat_pipeline.tool_use_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.tool_use_enabled),
        tool_use_max_calls: s("chat_pipeline.tool_use_max_calls")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.tool_use_max_calls),
        tool_use_llm: s("chat_pipeline.tool_use_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.tool_use_llm.clone()),
        // Feature: Adaptive Quality Thresholds
        adaptive_threshold_enabled: s("chat_pipeline.adaptive_threshold_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.adaptive_threshold_enabled),
        feedback_decay_days: s("chat_pipeline.feedback_decay_days")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.feedback_decay_days),
        adaptive_min_samples: s("chat_pipeline.adaptive_min_samples")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.adaptive_min_samples),
        // Self-RAG
        self_rag_enabled: s("chat_pipeline.self_rag_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.self_rag_enabled),
        self_rag_threshold: s("chat_pipeline.self_rag_threshold")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.self_rag_threshold),
        self_rag_llm: s("chat_pipeline.self_rag_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.self_rag_llm.clone()),
        // Graph RAG
        graph_rag_enabled: s("chat_pipeline.graph_rag_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.graph_rag_enabled),
        graph_rag_max_entities: s("chat_pipeline.graph_rag_max_entities")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.graph_rag_max_entities),
        graph_rag_max_depth: s("chat_pipeline.graph_rag_max_depth")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.graph_rag_max_depth),
        graph_rag_llm: s("chat_pipeline.graph_rag_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.graph_rag_llm.clone()),
        // CRAG
        crag_enabled: s("chat_pipeline.crag_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.crag_enabled),
        crag_relevance_threshold: s("chat_pipeline.crag_relevance_threshold")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.crag_relevance_threshold),
        crag_web_search_url: s("chat_pipeline.crag_web_search_url")
            .unwrap_or_else(|| cp.crag_web_search_url.clone()),
        crag_max_web_results: s("chat_pipeline.crag_max_web_results")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.crag_max_web_results),
        // Speculative RAG
        speculative_rag_enabled: s("chat_pipeline.speculative_rag_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.speculative_rag_enabled),
        speculative_candidates: s("chat_pipeline.speculative_candidates")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.speculative_candidates),
        speculative_rag_llm: s("chat_pipeline.speculative_rag_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.speculative_rag_llm.clone()),
        // Map-Reduce RAG
        map_reduce_enabled: s("chat_pipeline.map_reduce_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.map_reduce_enabled),
        map_reduce_max_chunks: s("chat_pipeline.map_reduce_max_chunks")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.map_reduce_max_chunks),
        map_reduce_llm: s("chat_pipeline.map_reduce_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.map_reduce_llm.clone()),
        // RAGAS
        ragas_enabled: s("chat_pipeline.ragas_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.ragas_enabled),
        ragas_sample_rate: s("chat_pipeline.ragas_sample_rate")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.ragas_sample_rate),
        ragas_llm: s("chat_pipeline.ragas_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.ragas_llm.clone()),
        // Contextual Compression
        compression_enabled: s("chat_pipeline.compression_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.compression_enabled),
        compression_target_ratio: s("chat_pipeline.compression_target_ratio")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.compression_target_ratio),
        compression_llm: s("chat_pipeline.compression_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.compression_llm.clone()),
        // Multi-modal RAG
        multimodal_enabled: s("chat_pipeline.multimodal_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.multimodal_enabled),
        multimodal_max_images: s("chat_pipeline.multimodal_max_images")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.multimodal_max_images),
        multimodal_llm: s("chat_pipeline.multimodal_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.multimodal_llm.clone()),
        // RAPTOR
        raptor_enabled: s("chat_pipeline.raptor_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.raptor_enabled),
        raptor_max_depth: s("chat_pipeline.raptor_max_depth")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.raptor_max_depth),
        raptor_group_size: s("chat_pipeline.raptor_group_size")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.raptor_group_size),
        raptor_llm: s("chat_pipeline.raptor_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.raptor_llm.clone()),
        // ColBERT
        colbert_enabled: s("chat_pipeline.colbert_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.colbert_enabled),
        colbert_top_n: s("chat_pipeline.colbert_top_n")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.colbert_top_n),
        colbert_llm: s("chat_pipeline.colbert_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.colbert_llm.clone()),
        // Active Learning
        active_learning_enabled: s("chat_pipeline.active_learning_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.active_learning_enabled),
        active_learning_min_interactions: s("chat_pipeline.active_learning_min_interactions")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.active_learning_min_interactions),
        active_learning_max_low_confidence: s("chat_pipeline.active_learning_max_low_confidence")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.active_learning_max_low_confidence),
        // LLM10: Budget cap
        max_llm_calls_per_request: s("chat_pipeline.max_llm_calls_per_request")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.max_llm_calls_per_request),
        // Per-LLM-call timeout
        request_timeout_secs: s("chat_pipeline.request_timeout_secs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.request_timeout_secs),
        // Ollama keep_alive
        ollama_keep_alive: s("chat_pipeline.ollama_keep_alive")
            .unwrap_or_else(|| cp.ollama_keep_alive.clone()),
        // Context Compaction
        context_compaction_enabled: s("chat_pipeline.context_compaction_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.context_compaction_enabled),
        model_context_window: s("chat_pipeline.model_context_window")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.model_context_window),
        compaction_threshold: s("chat_pipeline.compaction_threshold")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.compaction_threshold),
        compaction_keep_recent: s("chat_pipeline.compaction_keep_recent")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.compaction_keep_recent),
        // Conversation Summarization
        auto_summarize: s("chat_pipeline.auto_summarize")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.auto_summarize),
        summarize_threshold: s("chat_pipeline.summarize_threshold")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.summarize_threshold),
        summarize_keep_recent: s("chat_pipeline.summarize_keep_recent")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.summarize_keep_recent),
        // Personal Memory
        personal_memory_enabled: s("chat_pipeline.personal_memory_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.personal_memory_enabled),
        personal_memory_top_k: s("chat_pipeline.personal_memory_top_k")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.personal_memory_top_k),
        personal_memory_max_per_user: s("chat_pipeline.personal_memory_max_per_user")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.personal_memory_max_per_user),
        personal_memory_decay_factor: s("chat_pipeline.personal_memory_decay_factor")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.personal_memory_decay_factor),
        personal_memory_min_relevance: s("chat_pipeline.personal_memory_min_relevance")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.personal_memory_min_relevance),
        personal_memory_llm: s("chat_pipeline.personal_memory_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.personal_memory_llm.clone()),
        // Live Source Retrieval
        live_retrieval_enabled: s("chat_pipeline.live_retrieval_enabled")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.live_retrieval_enabled),
        live_retrieval_timeout_secs: s("chat_pipeline.live_retrieval_timeout_secs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.live_retrieval_timeout_secs),
        live_retrieval_max_connectors: s("chat_pipeline.live_retrieval_max_connectors")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.live_retrieval_max_connectors),
        live_retrieval_max_content_chars: s("chat_pipeline.live_retrieval_max_content_chars")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cp.live_retrieval_max_content_chars),
        live_retrieval_llm: s("chat_pipeline.live_retrieval_llm")
            .and_then(|v| serde_json::from_str(&v).ok())
            .or_else(|| cp.live_retrieval_llm.clone()),
    }
}

fn build_chat_pipeline_response_from_config(
    state: &AppState,
    eff: &thairag_config::schema::ChatPipelineConfig,
) -> ChatPipelineConfigResponse {
    let llm_mode = state
        .km_store
        .get_setting("chat_pipeline.llm_mode")
        .unwrap_or_else(|| "chat".to_string());
    ChatPipelineConfigResponse {
        enabled: eff.enabled,
        llm_mode,
        llm: eff.llm.as_ref().map(llm_config_to_info),
        query_analyzer_enabled: eff.query_analyzer_enabled,
        query_analyzer_llm: eff.query_analyzer_llm.as_ref().map(llm_config_to_info),
        query_rewriter_enabled: eff.query_rewriter_enabled,
        query_rewriter_llm: eff.query_rewriter_llm.as_ref().map(llm_config_to_info),
        context_curator_enabled: eff.context_curator_enabled,
        context_curator_llm: eff.context_curator_llm.as_ref().map(llm_config_to_info),
        response_generator_llm: eff.response_generator_llm.as_ref().map(llm_config_to_info),
        quality_guard_enabled: eff.quality_guard_enabled,
        quality_guard_llm: eff.quality_guard_llm.as_ref().map(llm_config_to_info),
        quality_guard_max_retries: eff.quality_guard_max_retries,
        quality_guard_threshold: eff.quality_guard_threshold,
        language_adapter_enabled: eff.language_adapter_enabled,
        language_adapter_llm: eff.language_adapter_llm.as_ref().map(llm_config_to_info),
        orchestrator_enabled: eff.orchestrator_enabled,
        max_orchestrator_calls: eff.max_orchestrator_calls,
        orchestrator_llm: eff.orchestrator_llm.as_ref().map(llm_config_to_info),
        max_context_tokens: eff.max_context_tokens,
        agent_max_tokens: eff.agent_max_tokens,
        request_timeout_secs: eff.request_timeout_secs,
        ollama_keep_alive: eff.ollama_keep_alive.clone(),
        conversation_memory_enabled: eff.conversation_memory_enabled,
        memory_max_summaries: eff.memory_max_summaries,
        memory_summary_max_tokens: eff.memory_summary_max_tokens,
        memory_llm: eff.memory_llm.as_ref().map(llm_config_to_info),
        retrieval_refinement_enabled: eff.retrieval_refinement_enabled,
        refinement_min_relevance: eff.refinement_min_relevance,
        refinement_max_retries: eff.refinement_max_retries,
        tool_use_enabled: eff.tool_use_enabled,
        tool_use_max_calls: eff.tool_use_max_calls,
        tool_use_llm: eff.tool_use_llm.as_ref().map(llm_config_to_info),
        adaptive_threshold_enabled: eff.adaptive_threshold_enabled,
        feedback_decay_days: eff.feedback_decay_days,
        adaptive_min_samples: eff.adaptive_min_samples,
        self_rag_enabled: eff.self_rag_enabled,
        self_rag_threshold: eff.self_rag_threshold,
        self_rag_llm: eff.self_rag_llm.as_ref().map(llm_config_to_info),
        graph_rag_enabled: eff.graph_rag_enabled,
        graph_rag_max_entities: eff.graph_rag_max_entities,
        graph_rag_max_depth: eff.graph_rag_max_depth,
        graph_rag_llm: eff.graph_rag_llm.as_ref().map(llm_config_to_info),
        crag_enabled: eff.crag_enabled,
        crag_relevance_threshold: eff.crag_relevance_threshold,
        crag_web_search_url: eff.crag_web_search_url.clone(),
        crag_max_web_results: eff.crag_max_web_results,
        speculative_rag_enabled: eff.speculative_rag_enabled,
        speculative_candidates: eff.speculative_candidates,
        speculative_rag_llm: eff.speculative_rag_llm.as_ref().map(llm_config_to_info),
        map_reduce_enabled: eff.map_reduce_enabled,
        map_reduce_max_chunks: eff.map_reduce_max_chunks,
        map_reduce_llm: eff.map_reduce_llm.as_ref().map(llm_config_to_info),
        ragas_enabled: eff.ragas_enabled,
        ragas_sample_rate: eff.ragas_sample_rate,
        ragas_llm: eff.ragas_llm.as_ref().map(llm_config_to_info),
        compression_enabled: eff.compression_enabled,
        compression_target_ratio: eff.compression_target_ratio,
        compression_llm: eff.compression_llm.as_ref().map(llm_config_to_info),
        multimodal_enabled: eff.multimodal_enabled,
        multimodal_max_images: eff.multimodal_max_images,
        multimodal_llm: eff.multimodal_llm.as_ref().map(llm_config_to_info),
        raptor_enabled: eff.raptor_enabled,
        raptor_max_depth: eff.raptor_max_depth,
        raptor_group_size: eff.raptor_group_size,
        raptor_llm: eff.raptor_llm.as_ref().map(llm_config_to_info),
        colbert_enabled: eff.colbert_enabled,
        colbert_top_n: eff.colbert_top_n,
        colbert_llm: eff.colbert_llm.as_ref().map(llm_config_to_info),
        active_learning_enabled: eff.active_learning_enabled,
        active_learning_min_interactions: eff.active_learning_min_interactions,
        active_learning_max_low_confidence: eff.active_learning_max_low_confidence,
        context_compaction_enabled: eff.context_compaction_enabled,
        model_context_window: eff.model_context_window,
        compaction_threshold: eff.compaction_threshold,
        compaction_keep_recent: eff.compaction_keep_recent,
        personal_memory_enabled: eff.personal_memory_enabled,
        personal_memory_top_k: eff.personal_memory_top_k,
        personal_memory_max_per_user: eff.personal_memory_max_per_user,
        personal_memory_decay_factor: eff.personal_memory_decay_factor,
        personal_memory_min_relevance: eff.personal_memory_min_relevance,
        personal_memory_llm: eff.personal_memory_llm.as_ref().map(llm_config_to_info),
        live_retrieval_enabled: eff.live_retrieval_enabled,
        live_retrieval_timeout_secs: eff.live_retrieval_timeout_secs,
        live_retrieval_max_connectors: eff.live_retrieval_max_connectors,
        live_retrieval_max_content_chars: eff.live_retrieval_max_content_chars,
        live_retrieval_llm: eff.live_retrieval_llm.as_ref().map(llm_config_to_info),
    }
}

fn build_chat_pipeline_response(state: &AppState) -> ChatPipelineConfigResponse {
    let eff = get_effective_chat_pipeline(state);
    build_chat_pipeline_response_from_config(state, &eff)
}

pub async fn get_chat_pipeline_config(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(sq): Query<ScopeQuery>,
) -> Result<Json<ChatPipelineConfigResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    let scope = parse_scope_query(&sq, &*state.km_store)?;
    if matches!(scope, SettingsScope::Global) {
        return Ok(Json(build_chat_pipeline_response(&state)));
    }
    let eff = get_effective_chat_pipeline_scoped(&state.config, &*state.km_store, &scope);
    Ok(Json(build_chat_pipeline_response_from_config(&state, &eff)))
}

pub async fn update_chat_pipeline_config(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(sq): Query<ScopeQuery>,
    AppJson(req): AppJson<UpdateChatPipelineRequest>,
) -> Result<Json<ChatPipelineConfigResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    let scope = parse_scope_query(&sq, &*state.km_store)?;
    let (scope_type, scope_id) = scope.as_pair();

    // Persist scalar settings (scoped)
    macro_rules! persist_bool {
        ($field:ident, $key:expr) => {
            if let Some(v) = req.$field {
                state
                    .km_store
                    .set_scoped_setting($key, scope_type, &scope_id, &v.to_string());
            }
        };
    }
    macro_rules! persist_num {
        ($field:ident, $key:expr) => {
            if let Some(v) = req.$field {
                state
                    .km_store
                    .set_scoped_setting($key, scope_type, &scope_id, &v.to_string());
            }
        };
    }

    if let Some(ref mode) = req.llm_mode {
        state
            .km_store
            .set_scoped_setting("chat_pipeline.llm_mode", scope_type, &scope_id, mode);
    }
    persist_bool!(enabled, "chat_pipeline.enabled");
    persist_bool!(
        query_analyzer_enabled,
        "chat_pipeline.query_analyzer_enabled"
    );
    persist_bool!(
        query_rewriter_enabled,
        "chat_pipeline.query_rewriter_enabled"
    );
    persist_bool!(
        context_curator_enabled,
        "chat_pipeline.context_curator_enabled"
    );
    persist_bool!(quality_guard_enabled, "chat_pipeline.quality_guard_enabled");
    persist_bool!(
        language_adapter_enabled,
        "chat_pipeline.language_adapter_enabled"
    );
    persist_bool!(orchestrator_enabled, "chat_pipeline.orchestrator_enabled");
    persist_num!(
        max_orchestrator_calls,
        "chat_pipeline.max_orchestrator_calls"
    );
    persist_num!(
        quality_guard_max_retries,
        "chat_pipeline.quality_guard_max_retries"
    );
    persist_num!(
        quality_guard_threshold,
        "chat_pipeline.quality_guard_threshold"
    );
    persist_num!(max_context_tokens, "chat_pipeline.max_context_tokens");
    persist_num!(agent_max_tokens, "chat_pipeline.agent_max_tokens");
    persist_num!(request_timeout_secs, "chat_pipeline.request_timeout_secs");
    if let Some(ref ka) = req.ollama_keep_alive {
        state.km_store.set_scoped_setting(
            "chat_pipeline.ollama_keep_alive",
            scope_type,
            &scope_id,
            ka,
        );
    }
    // Feature: Conversation Memory
    persist_bool!(
        conversation_memory_enabled,
        "chat_pipeline.conversation_memory_enabled"
    );
    persist_num!(memory_max_summaries, "chat_pipeline.memory_max_summaries");
    persist_num!(
        memory_summary_max_tokens,
        "chat_pipeline.memory_summary_max_tokens"
    );
    // Feature: Multi-turn Retrieval Refinement
    persist_bool!(
        retrieval_refinement_enabled,
        "chat_pipeline.retrieval_refinement_enabled"
    );
    persist_num!(
        refinement_min_relevance,
        "chat_pipeline.refinement_min_relevance"
    );
    persist_num!(
        refinement_max_retries,
        "chat_pipeline.refinement_max_retries"
    );
    // Feature: Agentic Tool Use
    persist_bool!(tool_use_enabled, "chat_pipeline.tool_use_enabled");
    persist_num!(tool_use_max_calls, "chat_pipeline.tool_use_max_calls");
    // Feature: Adaptive Quality Thresholds
    persist_bool!(
        adaptive_threshold_enabled,
        "chat_pipeline.adaptive_threshold_enabled"
    );
    persist_num!(feedback_decay_days, "chat_pipeline.feedback_decay_days");
    persist_num!(adaptive_min_samples, "chat_pipeline.adaptive_min_samples");
    // Self-RAG
    persist_bool!(self_rag_enabled, "chat_pipeline.self_rag_enabled");
    persist_num!(self_rag_threshold, "chat_pipeline.self_rag_threshold");
    // Graph RAG
    persist_bool!(graph_rag_enabled, "chat_pipeline.graph_rag_enabled");
    persist_num!(
        graph_rag_max_entities,
        "chat_pipeline.graph_rag_max_entities"
    );
    persist_num!(graph_rag_max_depth, "chat_pipeline.graph_rag_max_depth");
    // CRAG
    persist_bool!(crag_enabled, "chat_pipeline.crag_enabled");
    persist_num!(
        crag_relevance_threshold,
        "chat_pipeline.crag_relevance_threshold"
    );
    persist_num!(crag_max_web_results, "chat_pipeline.crag_max_web_results");
    if let Some(ref url) = req.crag_web_search_url {
        state.km_store.set_scoped_setting(
            "chat_pipeline.crag_web_search_url",
            scope_type,
            &scope_id,
            url,
        );
    }
    // Speculative RAG
    persist_bool!(
        speculative_rag_enabled,
        "chat_pipeline.speculative_rag_enabled"
    );
    persist_num!(
        speculative_candidates,
        "chat_pipeline.speculative_candidates"
    );
    // Map-Reduce RAG
    persist_bool!(map_reduce_enabled, "chat_pipeline.map_reduce_enabled");
    persist_num!(map_reduce_max_chunks, "chat_pipeline.map_reduce_max_chunks");
    // RAGAS
    persist_bool!(ragas_enabled, "chat_pipeline.ragas_enabled");
    persist_num!(ragas_sample_rate, "chat_pipeline.ragas_sample_rate");
    // Contextual Compression
    persist_bool!(compression_enabled, "chat_pipeline.compression_enabled");
    persist_num!(
        compression_target_ratio,
        "chat_pipeline.compression_target_ratio"
    );
    // Multi-modal RAG
    persist_bool!(multimodal_enabled, "chat_pipeline.multimodal_enabled");
    persist_num!(multimodal_max_images, "chat_pipeline.multimodal_max_images");
    // RAPTOR
    persist_bool!(raptor_enabled, "chat_pipeline.raptor_enabled");
    persist_num!(raptor_max_depth, "chat_pipeline.raptor_max_depth");
    persist_num!(raptor_group_size, "chat_pipeline.raptor_group_size");
    // ColBERT
    persist_bool!(colbert_enabled, "chat_pipeline.colbert_enabled");
    persist_num!(colbert_top_n, "chat_pipeline.colbert_top_n");
    // Active Learning
    persist_bool!(
        active_learning_enabled,
        "chat_pipeline.active_learning_enabled"
    );
    persist_num!(
        active_learning_min_interactions,
        "chat_pipeline.active_learning_min_interactions"
    );
    persist_num!(
        active_learning_max_low_confidence,
        "chat_pipeline.active_learning_max_low_confidence"
    );
    // Context Compaction
    persist_bool!(
        context_compaction_enabled,
        "chat_pipeline.context_compaction_enabled"
    );
    persist_num!(model_context_window, "chat_pipeline.model_context_window");
    persist_num!(compaction_threshold, "chat_pipeline.compaction_threshold");
    persist_num!(
        compaction_keep_recent,
        "chat_pipeline.compaction_keep_recent"
    );
    // Personal Memory
    persist_bool!(
        personal_memory_enabled,
        "chat_pipeline.personal_memory_enabled"
    );
    persist_num!(personal_memory_top_k, "chat_pipeline.personal_memory_top_k");
    persist_num!(
        personal_memory_max_per_user,
        "chat_pipeline.personal_memory_max_per_user"
    );
    persist_num!(
        personal_memory_decay_factor,
        "chat_pipeline.personal_memory_decay_factor"
    );
    persist_num!(
        personal_memory_min_relevance,
        "chat_pipeline.personal_memory_min_relevance"
    );
    // Live Source Retrieval
    persist_bool!(
        live_retrieval_enabled,
        "chat_pipeline.live_retrieval_enabled"
    );
    persist_num!(
        live_retrieval_timeout_secs,
        "chat_pipeline.live_retrieval_timeout_secs"
    );
    persist_num!(
        live_retrieval_max_connectors,
        "chat_pipeline.live_retrieval_max_connectors"
    );
    persist_num!(
        live_retrieval_max_content_chars,
        "chat_pipeline.live_retrieval_max_content_chars"
    );

    // Helper: persist LLM config (scoped)
    fn persist_chat_llm(
        state: &AppState,
        key: &str,
        update: &UpdateLlmConfig,
        current: Option<thairag_config::schema::LlmConfig>,
        scope_type: &str,
        scope_id: &str,
    ) -> Result<(), ApiError> {
        use thairag_core::types::LlmKind;

        let mut cfg = current.unwrap_or_else(|| state.config.providers.llm.clone());
        if let Some(kind) = &update.kind {
            let new_kind =
                parse_llm_kind(kind).map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
            // When provider kind changes, reset base_url and api_key to avoid
            // sending requests to the wrong endpoint (e.g. Ollama URL for OpenAI).
            if new_kind != cfg.kind {
                cfg.kind = new_kind;
                cfg.base_url = String::new(); // let each provider use its default
                cfg.api_key = match cfg.kind {
                    LlmKind::OpenAi
                    | LlmKind::OpenAiCompatible
                    | LlmKind::Claude
                    | LlmKind::Gemini => state.config.providers.llm.api_key.clone(),
                    LlmKind::Ollama => String::new(),
                };
            }
        }
        if let Some(model) = &update.model {
            cfg.model = model.clone();
        }
        if let Some(base_url) = &update.base_url {
            cfg.base_url = base_url.clone();
        }
        if let Some(api_key) = &update.api_key
            && !api_key.is_empty()
        {
            cfg.api_key = api_key.clone();
        }
        if let Some(max_tokens) = update.max_tokens {
            cfg.max_tokens = Some(max_tokens);
        }
        // Profile ID management
        if update.clear_profile == Some(true) {
            cfg.profile_id = None;
        } else if let Some(ref pid) = update.profile_id {
            cfg.profile_id = Some(pid.clone());
        }
        let json = serde_json::to_string(&cfg)
            .map_err(|e| ApiError(ThaiRagError::Internal(format!("Serialize: {e}"))))?;
        state
            .km_store
            .set_scoped_setting(key, scope_type, scope_id, &json);
        Ok(())
    }

    // Persist LLM configs (scoped)
    let eff = get_effective_chat_pipeline(&state);
    macro_rules! persist_llm {
        ($field:ident, $key:expr, $eff_field:expr) => {
            if let Some(ref u) = req.$field {
                persist_chat_llm(&state, $key, u, $eff_field, scope_type, &scope_id)?;
            }
        };
    }
    persist_llm!(llm, "chat_pipeline.llm", eff.llm.clone());
    persist_llm!(
        query_analyzer_llm,
        "chat_pipeline.query_analyzer_llm",
        eff.query_analyzer_llm.clone()
    );
    persist_llm!(
        query_rewriter_llm,
        "chat_pipeline.query_rewriter_llm",
        eff.query_rewriter_llm.clone()
    );
    persist_llm!(
        context_curator_llm,
        "chat_pipeline.context_curator_llm",
        eff.context_curator_llm.clone()
    );
    persist_llm!(
        response_generator_llm,
        "chat_pipeline.response_generator_llm",
        eff.response_generator_llm.clone()
    );
    persist_llm!(
        quality_guard_llm,
        "chat_pipeline.quality_guard_llm",
        eff.quality_guard_llm.clone()
    );
    persist_llm!(
        language_adapter_llm,
        "chat_pipeline.language_adapter_llm",
        eff.language_adapter_llm.clone()
    );
    persist_llm!(
        orchestrator_llm,
        "chat_pipeline.orchestrator_llm",
        eff.orchestrator_llm.clone()
    );
    persist_llm!(
        memory_llm,
        "chat_pipeline.memory_llm",
        eff.memory_llm.clone()
    );
    persist_llm!(
        tool_use_llm,
        "chat_pipeline.tool_use_llm",
        eff.tool_use_llm.clone()
    );
    persist_llm!(
        self_rag_llm,
        "chat_pipeline.self_rag_llm",
        eff.self_rag_llm.clone()
    );
    persist_llm!(
        graph_rag_llm,
        "chat_pipeline.graph_rag_llm",
        eff.graph_rag_llm.clone()
    );
    persist_llm!(
        map_reduce_llm,
        "chat_pipeline.map_reduce_llm",
        eff.map_reduce_llm.clone()
    );
    persist_llm!(
        speculative_rag_llm,
        "chat_pipeline.speculative_rag_llm",
        eff.speculative_rag_llm.clone()
    );
    persist_llm!(ragas_llm, "chat_pipeline.ragas_llm", eff.ragas_llm.clone());
    persist_llm!(
        compression_llm,
        "chat_pipeline.compression_llm",
        eff.compression_llm.clone()
    );
    persist_llm!(
        multimodal_llm,
        "chat_pipeline.multimodal_llm",
        eff.multimodal_llm.clone()
    );
    persist_llm!(
        raptor_llm,
        "chat_pipeline.raptor_llm",
        eff.raptor_llm.clone()
    );
    persist_llm!(
        colbert_llm,
        "chat_pipeline.colbert_llm",
        eff.colbert_llm.clone()
    );
    persist_llm!(
        personal_memory_llm,
        "chat_pipeline.personal_memory_llm",
        eff.personal_memory_llm.clone()
    );
    persist_llm!(
        live_retrieval_llm,
        "chat_pipeline.live_retrieval_llm",
        eff.live_retrieval_llm.clone()
    );

    // Handle removal of LLM overrides (scoped)
    macro_rules! remove_llm {
        ($llm_field:ident, $remove_field:ident, $key:expr) => {
            if req.$llm_field.is_none() && req.$remove_field.unwrap_or(false) {
                state
                    .km_store
                    .delete_scoped_setting($key, scope_type, &scope_id);
            }
        };
    }
    remove_llm!(llm, remove_llm, "chat_pipeline.llm");
    remove_llm!(
        query_analyzer_llm,
        remove_query_analyzer_llm,
        "chat_pipeline.query_analyzer_llm"
    );
    remove_llm!(
        query_rewriter_llm,
        remove_query_rewriter_llm,
        "chat_pipeline.query_rewriter_llm"
    );
    remove_llm!(
        context_curator_llm,
        remove_context_curator_llm,
        "chat_pipeline.context_curator_llm"
    );
    remove_llm!(
        response_generator_llm,
        remove_response_generator_llm,
        "chat_pipeline.response_generator_llm"
    );
    remove_llm!(
        quality_guard_llm,
        remove_quality_guard_llm,
        "chat_pipeline.quality_guard_llm"
    );
    remove_llm!(
        language_adapter_llm,
        remove_language_adapter_llm,
        "chat_pipeline.language_adapter_llm"
    );
    remove_llm!(
        orchestrator_llm,
        remove_orchestrator_llm,
        "chat_pipeline.orchestrator_llm"
    );
    remove_llm!(memory_llm, remove_memory_llm, "chat_pipeline.memory_llm");
    remove_llm!(
        tool_use_llm,
        remove_tool_use_llm,
        "chat_pipeline.tool_use_llm"
    );
    remove_llm!(
        self_rag_llm,
        remove_self_rag_llm,
        "chat_pipeline.self_rag_llm"
    );
    remove_llm!(
        graph_rag_llm,
        remove_graph_rag_llm,
        "chat_pipeline.graph_rag_llm"
    );
    remove_llm!(
        map_reduce_llm,
        remove_map_reduce_llm,
        "chat_pipeline.map_reduce_llm"
    );
    remove_llm!(ragas_llm, remove_ragas_llm, "chat_pipeline.ragas_llm");
    remove_llm!(
        compression_llm,
        remove_compression_llm,
        "chat_pipeline.compression_llm"
    );
    remove_llm!(
        multimodal_llm,
        remove_multimodal_llm,
        "chat_pipeline.multimodal_llm"
    );
    remove_llm!(raptor_llm, remove_raptor_llm, "chat_pipeline.raptor_llm");
    remove_llm!(colbert_llm, remove_colbert_llm, "chat_pipeline.colbert_llm");
    remove_llm!(
        personal_memory_llm,
        remove_personal_memory_llm,
        "chat_pipeline.personal_memory_llm"
    );
    remove_llm!(
        live_retrieval_llm,
        remove_live_retrieval_llm,
        "chat_pipeline.live_retrieval_llm"
    );
    remove_llm!(
        speculative_rag_llm,
        remove_speculative_rag_llm,
        "chat_pipeline.speculative_rag_llm"
    );

    // Hot-reload only for global scope (scoped settings are resolved at request time)
    if matches!(scope, SettingsScope::Global) {
        let eff_chat = get_effective_chat_pipeline(&state);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            state.build_provider_bundle(
                &state.providers().providers_config,
                &state.config.search,
                &state.config.document,
                &eff_chat,
            )
        })) {
            Ok(bundle) => {
                state.reload_providers(bundle);
                tracing::info!("Chat pipeline config updated and hot-reloaded");
            }
            Err(e) => {
                tracing::warn!("Hot-reload failed after chat pipeline save: {:?}", e);
            }
        }
    }

    let eff_response = get_effective_chat_pipeline_scoped(&state.config, &*state.km_store, &scope);
    Ok(Json(build_chat_pipeline_response_from_config(
        &state,
        &eff_response,
    )))
}

// ── Scoped Settings Info ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct ScopeInfoResponse {
    pub scope_type: String,
    pub scope_id: String,
    pub overrides: std::collections::HashMap<String, Vec<String>>,
}

pub async fn get_scope_info(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(sq): Query<ScopeQuery>,
) -> Result<Json<ScopeInfoResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    let scope = parse_scope_query(&sq, &*state.km_store)?;
    let (st, si) = scope.as_pair();

    let mut overrides = std::collections::HashMap::new();
    for (scope_type, scope_id) in scope.inheritance_chain() {
        let keys = state.km_store.list_override_keys(scope_type, &scope_id);
        overrides.insert(scope_type.to_string(), keys);
    }

    Ok(Json(ScopeInfoResponse {
        scope_type: st.to_string(),
        scope_id: si,
        overrides,
    }))
}

#[derive(Deserialize)]
pub struct ResetScopeQuery {
    pub scope_type: Option<String>,
    pub scope_id: Option<String>,
    pub key: Option<String>,
}

pub async fn reset_scoped_setting(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(sq): Query<ResetScopeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;
    let scope = parse_scope_query(
        &ScopeQuery {
            scope_type: sq.scope_type,
            scope_id: sq.scope_id,
        },
        &*state.km_store,
    )?;

    if matches!(scope, SettingsScope::Global) {
        return Err(ApiError(ThaiRagError::Validation(
            "Cannot reset global scope settings. Use the specific PUT endpoint instead.".into(),
        )));
    }

    let (scope_type, scope_id) = scope.as_pair();

    if let Some(key) = &sq.key {
        state
            .km_store
            .delete_scoped_setting(key, scope_type, &scope_id);
        Ok(Json(serde_json::json!({ "status": "reset", "key": key })))
    } else {
        state
            .km_store
            .delete_all_scoped_settings(scope_type, &scope_id);
        Ok(Json(
            serde_json::json!({ "status": "reset_all", "scope_type": scope_type, "scope_id": scope_id }),
        ))
    }
}

// ── Presets ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PresetModelInfo {
    pub model: String,
    pub role: String,
    pub task_weight: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct PresetInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// "chat" or "document"
    pub category: String,
    pub required_models: Vec<PresetModelInfo>,
    /// Non-model settings summary (e.g. reranker, chunk size, tuning params)
    pub settings_summary: Vec<SettingsSummaryItem>,
    /// Cost/performance metadata for UI display
    pub estimated_cost_per_query: String,
    pub estimated_latency: String,
    pub llm_calls_per_query: String,
    pub feature_count: u32,
    /// "ollama" or "cloud"
    pub provider_type: String,
    /// List of enabled feature names (e.g. "Conversation Memory", "Graph RAG")
    pub features: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct SettingsSummaryItem {
    pub label: String,
    pub value: String,
}

/// GET /api/km/settings/presets
pub async fn list_presets(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<PresetInfo>>, ApiError> {
    require_super_admin(&claims, &state)?;
    Ok(Json(get_preset_definitions()))
}

fn get_preset_definitions() -> Vec<PresetInfo> {
    // All presets use Thai-capable models only.
    // Thai support: Qwen3 (119 languages), Chinda (Thai-optimized Qwen3-4B),
    //   Llama4 Scout (Thai in supported languages), qwen3-embedding (100+ languages)
    fn s(label: &str, value: &str) -> SettingsSummaryItem {
        SettingsSummaryItem {
            label: label.into(),
            value: value.into(),
        }
    }

    vec![
        // ── Chat & Response Pipeline presets ──
        PresetInfo {
            id: "thai-basic".into(),
            name: "Thai Basic (เริ่มต้น)".into(),
            description: "Essential Thai RAG with Chinda — a Thai-optimized 4B model by iApp. Low resource usage, great for getting started.".into(),
            category: "chat".into(),
            required_models: vec![
                PresetModelInfo { model: "iapp/chinda-qwen3-4b".into(), role: "Main LLM".into(), task_weight: "heavy".into(), description: "Thai-optimized Qwen3-4B — chat, curation, quality guard".into() },
                PresetModelInfo { model: "qwen3-embedding:0.6b".into(), role: "Embedding".into(), task_weight: "light".into(), description: "Multilingual embeddings with Thai support (dim=1024)".into() },
            ],
            settings_summary: vec![
                s("LLM Mode", "Shared (single model)"),
                s("Reranker", "Passthrough"),
                s("Context Window", "2,048 tokens"),
                s("Agent Max Tokens", "1,024"),
                s("Quality Guard", "Threshold 0.5 / No retry"),
                s("Agents", "5 core agents (no orchestrator)"),
                s("Features", "None (minimal setup)"),
            ],
            estimated_cost_per_query: "Free".into(),
            estimated_latency: "15-30s".into(),
            llm_calls_per_query: "5 calls".into(),
            feature_count: 0,
            provider_type: "ollama".into(),
            features: vec![],
        },
        PresetInfo {
            id: "thai-recommended".into(),
            name: "Thai Recommended (แนะนำ)".into(),
            description: "Best balance of quality and speed. Qwen3-14B with Thai comprehension, orchestrator, memory, and ColBERT.".into(),
            category: "chat".into(),
            required_models: vec![
                PresetModelInfo { model: "qwen3:14b".into(), role: "Main LLM".into(), task_weight: "heavy".into(), description: "119 languages incl. Thai — chat, curation, quality guard".into() },
                PresetModelInfo { model: "qwen3-embedding:8b".into(), role: "Embedding".into(), task_weight: "light".into(), description: "#1 MTEB multilingual, Thai-capable (dim=4096)".into() },
            ],
            settings_summary: vec![
                s("LLM Mode", "Shared (single model)"),
                s("Reranker", "Passthrough"),
                s("Context Window", "4,096 tokens"),
                s("Agent Max Tokens", "2,048"),
                s("Quality Guard", "Threshold 0.6 / 1 retry"),
                s("Orchestrator", "Enabled (max 3 calls)"),
                s("Agents", "6 agents (all core + orchestrator)"),
                s("Features", "Conversation Memory, ColBERT, Active Learning"),
            ],
            estimated_cost_per_query: "Free".into(),
            estimated_latency: "30-60s".into(),
            llm_calls_per_query: "5-7 calls".into(),
            feature_count: 3,
            provider_type: "ollama".into(),
            features: vec![
                "Conversation Memory".into(),
                "ColBERT Reranking".into(),
                "Active Learning".into(),
            ],
        },
        PresetInfo {
            id: "thai-max".into(),
            name: "Thai Maximum (สูงสุด)".into(),
            description: "All features with dedicated models per task. Best quality, highest resource usage. Requires 128GB+ VRAM.".into(),
            category: "chat".into(),
            required_models: vec![
                PresetModelInfo { model: "qwen3:32b".into(), role: "Main LLM (Heavy)".into(), task_weight: "heavy".into(), description: "Best Thai quality — response generation, quality guard, curation".into() },
                PresetModelInfo { model: "qwen3:14b".into(), role: "Agent LLM (Medium)".into(), task_weight: "medium".into(), description: "Graph RAG, CRAG, RAPTOR, ColBERT, compression, memory".into() },
                PresetModelInfo { model: "iapp/chinda-qwen3-4b".into(), role: "Light LLM".into(), task_weight: "light".into(), description: "Thai-optimized fast model — query analysis, rewriting".into() },
                PresetModelInfo { model: "llama4:scout".into(), role: "Vision LLM".into(), task_weight: "medium".into(), description: "Multimodal with Thai support — image descriptions".into() },
                PresetModelInfo { model: "qwen3-embedding:8b".into(), role: "Embedding".into(), task_weight: "light".into(), description: "#1 MTEB multilingual, Thai-capable (dim=4096)".into() },
            ],
            settings_summary: vec![
                s("LLM Mode", "Per-agent (dedicated models)"),
                s("Reranker", "Passthrough"),
                s("Context Window", "8,192 tokens"),
                s("Agent Max Tokens", "4,096"),
                s("Quality Guard", "Threshold 0.7 / 2 retries"),
                s("Orchestrator", "Enabled (max 5 calls)"),
                s("Agents", "6 agents (all core + orchestrator)"),
                s("Features", "All 13+ features enabled"),
            ],
            estimated_cost_per_query: "Free".into(),
            estimated_latency: "2-5 min".into(),
            llm_calls_per_query: "10-15 calls".into(),
            feature_count: 16,
            provider_type: "ollama".into(),
            features: vec![
                "Conversation Memory".into(),
                "Retrieval Refinement".into(),
                "Agentic Tool Use".into(),
                "Adaptive Threshold".into(),
                "Self-RAG".into(),
                "Graph RAG".into(),
                "Corrective RAG (CRAG)".into(),
                "Contextual Compression".into(),
                "Multimodal RAG".into(),
                "RAPTOR Summaries".into(),
                "ColBERT Reranking".into(),
                "Active Learning".into(),
                "Map-Reduce RAG".into(),
                "RAGAS Evaluation".into(),
                "Personal Memory".into(),
                "Live Source Retrieval".into(),
            ],
        },
        // ── Cloud Chat presets (OpenAI API) ──
        PresetInfo {
            id: "cloud-basic".into(),
            name: "Cloud Basic".into(),
            description: "Fast and affordable cloud RAG with GPT-4.1 Mini. No GPU needed — just an OpenAI API key.".into(),
            category: "chat".into(),
            required_models: vec![
                PresetModelInfo { model: "gpt-4.1-mini".into(), role: "Main LLM".into(), task_weight: "heavy".into(), description: "Fast, affordable — chat, curation, quality guard".into() },
            ],
            settings_summary: vec![
                s("LLM Mode", "Shared (single model)"),
                s("Embedding", "FastEmbed (local, no API key)"),
                s("Context Window", "4,096 tokens"),
                s("Agent Max Tokens", "2,048"),
                s("Quality Guard", "Threshold 0.6 / No retry"),
                s("Agents", "5 core agents (no orchestrator)"),
                s("Features", "None (minimal setup)"),
            ],
            estimated_cost_per_query: "~$0.003".into(),
            estimated_latency: "2-5s".into(),
            llm_calls_per_query: "5 calls".into(),
            feature_count: 0,
            provider_type: "cloud".into(),
            features: vec![],
        },
        PresetInfo {
            id: "cloud-recommended".into(),
            name: "Cloud Recommended".into(),
            description: "Best balance of cost and quality. GPT-4.1 Mini with orchestrator, memory, and smart features.".into(),
            category: "chat".into(),
            required_models: vec![
                PresetModelInfo { model: "gpt-4.1-mini".into(), role: "Main LLM".into(), task_weight: "heavy".into(), description: "Shared LLM for all agents".into() },
            ],
            settings_summary: vec![
                s("LLM Mode", "Shared (single model)"),
                s("Embedding", "FastEmbed (local, no API key)"),
                s("Context Window", "4,096 tokens"),
                s("Agent Max Tokens", "2,048"),
                s("Quality Guard", "Threshold 0.6 / 1 retry"),
                s("Orchestrator", "Enabled (max 3 calls)"),
                s("Agents", "6 agents (all core + orchestrator)"),
                s("Features", "Memory, ColBERT, Active Learning, Adaptive Threshold"),
            ],
            estimated_cost_per_query: "~$0.01".into(),
            estimated_latency: "3-8s".into(),
            llm_calls_per_query: "5-7 calls".into(),
            feature_count: 4,
            provider_type: "cloud".into(),
            features: vec![
                "Conversation Memory".into(),
                "ColBERT Reranking".into(),
                "Active Learning".into(),
                "Adaptive Threshold".into(),
            ],
        },
        PresetInfo {
            id: "cloud-max".into(),
            name: "Cloud Maximum".into(),
            description: "Full power with dedicated cloud models per task. GPT-4.1 for heavy work, Mini for medium, Nano for light tasks.".into(),
            category: "chat".into(),
            required_models: vec![
                PresetModelInfo { model: "gpt-4.1".into(), role: "Main LLM (Heavy)".into(), task_weight: "heavy".into(), description: "Response generation, quality guard, curation".into() },
                PresetModelInfo { model: "gpt-4.1-mini".into(), role: "Agent LLM (Medium)".into(), task_weight: "medium".into(), description: "Graph RAG, CRAG, RAPTOR, compression, memory".into() },
                PresetModelInfo { model: "gpt-4.1-nano".into(), role: "Light LLM".into(), task_weight: "light".into(), description: "Query analysis, rewriting, language adapter".into() },
            ],
            settings_summary: vec![
                s("LLM Mode", "Per-agent (dedicated models)"),
                s("Embedding", "FastEmbed (local, no API key)"),
                s("Context Window", "8,192 tokens"),
                s("Agent Max Tokens", "4,096"),
                s("Quality Guard", "Threshold 0.7 / 2 retries"),
                s("Orchestrator", "Enabled (max 5 calls)"),
                s("Agents", "6 agents (all core + orchestrator)"),
                s("Features", "All 13+ features enabled"),
            ],
            estimated_cost_per_query: "~$0.05-0.15".into(),
            estimated_latency: "5-15s".into(),
            llm_calls_per_query: "10-15 calls".into(),
            feature_count: 16,
            provider_type: "cloud".into(),
            features: vec![
                "Conversation Memory".into(),
                "Retrieval Refinement".into(),
                "Agentic Tool Use".into(),
                "Adaptive Threshold".into(),
                "Self-RAG".into(),
                "Graph RAG".into(),
                "Corrective RAG (CRAG)".into(),
                "Contextual Compression".into(),
                "Multimodal RAG".into(),
                "RAPTOR Summaries".into(),
                "ColBERT Reranking".into(),
                "Active Learning".into(),
                "Map-Reduce RAG".into(),
                "RAGAS Evaluation".into(),
                "Personal Memory".into(),
                "Live Source Retrieval".into(),
            ],
        },
        // ── Document Processing presets ──
        PresetInfo {
            id: "thai-doc-basic".into(),
            name: "Thai Doc Basic (เอกสารเริ่มต้น)".into(),
            description: "Lightweight document processing with Chinda for Thai text analysis, chunking, and enrichment.".into(),
            category: "document".into(),
            required_models: vec![
                PresetModelInfo { model: "iapp/chinda-qwen3-4b".into(), role: "Document AI + Enricher".into(), task_weight: "light".into(), description: "Thai-optimized analysis, chunking, enrichment".into() },
                PresetModelInfo { model: "qwen3-embedding:0.6b".into(), role: "Embedding".into(), task_weight: "light".into(), description: "Multilingual embeddings with Thai support (dim=1024)".into() },
            ],
            settings_summary: vec![
                s("Chunk Size", "512 chars / 64 overlap"),
                s("Embedding Dim", "1,024"),
                s("AI Agent Tokens", "1,024"),
                s("Max LLM Input", "4,000 chars"),
                s("Quality Threshold", "0.5"),
                s("Enricher", "Enabled"),
                s("Orchestrator", "Disabled"),
            ],
            estimated_cost_per_query: "Free".into(),
            estimated_latency: "5-15s/page".into(),
            llm_calls_per_query: "2-3 calls/page".into(),
            feature_count: 1,
            provider_type: "ollama".into(),
            features: vec![
                "AI Enrichment".into(),
            ],
        },
        PresetInfo {
            id: "thai-doc-recommended".into(),
            name: "Thai Doc Recommended (เอกสารแนะนำ)".into(),
            description: "Best document processing — Qwen3-14B for analysis + Chinda for enrichment + full embedding.".into(),
            category: "document".into(),
            required_models: vec![
                PresetModelInfo { model: "qwen3:14b".into(), role: "Document AI".into(), task_weight: "heavy".into(), description: "Thai-capable analysis, conversion, quality check, chunking".into() },
                PresetModelInfo { model: "iapp/chinda-qwen3-4b".into(), role: "Enricher".into(), task_weight: "light".into(), description: "Thai-optimized metadata enrichment".into() },
                PresetModelInfo { model: "qwen3-embedding:8b".into(), role: "Embedding".into(), task_weight: "light".into(), description: "#1 MTEB multilingual, Thai-capable (dim=4096)".into() },
            ],
            settings_summary: vec![
                s("Chunk Size", "1,024 chars / 128 overlap"),
                s("Embedding Dim", "4,096"),
                s("AI Agent Tokens", "2,048"),
                s("Max LLM Input", "8,000 chars"),
                s("Quality Threshold", "0.7"),
                s("Enricher", "Enabled"),
                s("Orchestrator", "Enabled (auto budget, max 3 calls)"),
            ],
            estimated_cost_per_query: "Free".into(),
            estimated_latency: "10-30s/page".into(),
            llm_calls_per_query: "3-5 calls/page".into(),
            feature_count: 2,
            provider_type: "ollama".into(),
            features: vec![
                "AI Enrichment".into(),
                "Orchestrator".into(),
            ],
        },
        // ── Cloud Document Processing presets ──
        PresetInfo {
            id: "cloud-doc-basic".into(),
            name: "Cloud Doc Basic".into(),
            description: "Fast document processing with GPT-4.1 Mini. No GPU needed.".into(),
            category: "document".into(),
            required_models: vec![
                PresetModelInfo { model: "gpt-4.1-mini".into(), role: "Document AI + Enricher".into(), task_weight: "light".into(), description: "Analysis, chunking, enrichment".into() },
            ],
            settings_summary: vec![
                s("Chunk Size", "512 chars / 64 overlap"),
                s("Embedding", "FastEmbed (local, no API key)"),
                s("AI Agent Tokens", "1,024"),
                s("Max LLM Input", "4,000 chars"),
                s("Quality Threshold", "0.5"),
                s("Enricher", "Enabled"),
                s("Orchestrator", "Disabled"),
            ],
            estimated_cost_per_query: "~$0.005/page".into(),
            estimated_latency: "1-3s/page".into(),
            llm_calls_per_query: "2-3 calls/page".into(),
            feature_count: 1,
            provider_type: "cloud".into(),
            features: vec![
                "AI Enrichment".into(),
            ],
        },
        PresetInfo {
            id: "cloud-doc-recommended".into(),
            name: "Cloud Doc Recommended".into(),
            description: "Best cloud document processing — GPT-4.1 for analysis + Mini for enrichment.".into(),
            category: "document".into(),
            required_models: vec![
                PresetModelInfo { model: "gpt-4.1".into(), role: "Document AI".into(), task_weight: "heavy".into(), description: "Analysis, conversion, quality check".into() },
                PresetModelInfo { model: "gpt-4.1-mini".into(), role: "Enricher".into(), task_weight: "light".into(), description: "Metadata enrichment".into() },
            ],
            settings_summary: vec![
                s("Chunk Size", "1,024 chars / 128 overlap"),
                s("Embedding", "FastEmbed (local, no API key)"),
                s("AI Agent Tokens", "2,048"),
                s("Max LLM Input", "8,000 chars"),
                s("Quality Threshold", "0.7"),
                s("Enricher", "Enabled"),
                s("Orchestrator", "Enabled (auto budget, max 3 calls)"),
            ],
            estimated_cost_per_query: "~$0.02/page".into(),
            estimated_latency: "2-5s/page".into(),
            llm_calls_per_query: "3-5 calls/page".into(),
            feature_count: 2,
            provider_type: "cloud".into(),
            features: vec![
                "AI Enrichment".into(),
                "Orchestrator".into(),
            ],
        },
    ]
}

#[derive(Deserialize)]
pub struct ApplyPresetRequest {
    pub preset_id: String,
    /// Ollama base URL to use for all LLM configs
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    /// API key for cloud presets (OpenAI, etc.)
    #[serde(default)]
    pub api_key: String,
}

fn default_ollama_url() -> String {
    "http://host.docker.internal:11435".into()
}

/// POST /api/km/settings/presets/apply
pub async fn apply_preset(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<ApplyPresetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    let url = req.ollama_url.clone();
    let api_key = req.api_key.clone();
    let store = &state.km_store;

    fn ollama_llm(model: &str, url: &str) -> String {
        serde_json::json!({
            "kind": "ollama",
            "model": model,
            "base_url": url,
            "api_key": ""
        })
        .to_string()
    }

    #[allow(dead_code)] // kept for backward compatibility
    fn cloud_llm(model: &str, api_key: &str) -> String {
        serde_json::json!({
            "kind": "openai",
            "model": model,
            "base_url": "",
            "api_key": api_key
        })
        .to_string()
    }

    // ── Vault-backed cloud helpers ─────────────────────────────────
    // Ensure a single vault key exists for the provider, returning its ID.
    let ensure_vault_key = |api_key: &str, provider_name: &str| -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let existing = store.list_vault_keys();
        let existing_row = existing.iter().find(|k| k.name == provider_name);

        let key_prefix = if api_key.len() >= 4 {
            api_key[..4].to_string()
        } else {
            api_key.to_string()
        };
        let key_suffix = if api_key.len() >= 4 {
            api_key[api_key.len() - 4..].to_string()
        } else {
            api_key.to_string()
        };
        let encrypted = state.vault.encrypt(api_key);

        if let Some(row) = existing_row {
            // Update the existing vault key with the new encrypted key
            let updated = crate::store::VaultKeyRow {
                id: row.id.clone(),
                name: provider_name.to_string(),
                provider: "openai".to_string(),
                encrypted_key: encrypted,
                key_prefix,
                key_suffix,
                base_url: String::new(),
                created_at: row.created_at.clone(),
                updated_at: now,
            };
            store.upsert_vault_key(&updated);
            row.id.clone()
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let row = crate::store::VaultKeyRow {
                id: id.clone(),
                name: provider_name.to_string(),
                provider: "openai".to_string(),
                encrypted_key: encrypted,
                key_prefix,
                key_suffix,
                base_url: String::new(),
                created_at: now.clone(),
                updated_at: now,
            };
            store.upsert_vault_key(&row);
            id
        }
    };

    // Ensure an LLM profile exists for the given model, returning a JSON
    // config string with profile_id instead of raw api_key.
    let ensure_llm_profile = |vault_key_id: &str, name: &str, model: &str| -> String {
        let now = chrono::Utc::now().to_rfc3339();
        let existing = store.list_llm_profiles();
        let existing_row = existing.iter().find(|p| p.name == name);

        let profile_id = if let Some(row) = existing_row {
            let updated = crate::store::LlmProfileRow {
                id: row.id.clone(),
                name: name.to_string(),
                kind: "openai".to_string(),
                model: model.to_string(),
                base_url: String::new(),
                vault_key_id: Some(vault_key_id.to_string()),
                max_tokens: None,
                created_at: row.created_at.clone(),
                updated_at: now,
            };
            store.upsert_llm_profile(&updated);
            row.id.clone()
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let row = crate::store::LlmProfileRow {
                id: id.clone(),
                name: name.to_string(),
                kind: "openai".to_string(),
                model: model.to_string(),
                base_url: String::new(),
                vault_key_id: Some(vault_key_id.to_string()),
                max_tokens: None,
                created_at: now.clone(),
                updated_at: now,
            };
            store.upsert_llm_profile(&row);
            id
        };

        serde_json::json!({
            "kind": "openai",
            "model": model,
            "profile_id": profile_id
        })
        .to_string()
    };

    match req.preset_id.as_str() {
        "thai-basic" => {
            // ── LLM: shared mode with Chinda (Thai-optimized 4B) ──
            store.set_setting("chat_pipeline.enabled", "true");
            store.set_setting("chat_pipeline.llm_mode", "shared");
            store.set_setting(
                "chat_pipeline.llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            // ── Agents ──
            store.set_setting("chat_pipeline.query_analyzer_enabled", "true");
            store.set_setting("chat_pipeline.query_rewriter_enabled", "true");
            store.set_setting("chat_pipeline.context_curator_enabled", "true");
            store.set_setting("chat_pipeline.quality_guard_enabled", "true");
            store.set_setting("chat_pipeline.language_adapter_enabled", "true");
            store.set_setting("chat_pipeline.orchestrator_enabled", "false");
            // ── Embedding ──
            store.set_setting("providers.embedding.kind", "ollama");
            store.set_setting("providers.embedding.model", "qwen3-embedding:0.6b");
            store.set_setting("providers.embedding.base_url", &url);
            store.set_setting("providers.embedding.dimensions", "1024");
            // ── Reranker: passthrough (no external service needed) ──
            store.set_setting("providers.reranker.kind", "passthrough");
            // ── Tuning: conservative for 4B model ──
            store.set_setting("chat_pipeline.max_context_tokens", "2048");
            store.set_setting("chat_pipeline.agent_max_tokens", "1024");
            store.set_setting("chat_pipeline.quality_guard_threshold", "0.5");
            store.set_setting("chat_pipeline.quality_guard_max_retries", "0");
            // ── Disable all advanced features ──
            for feat in &[
                "conversation_memory",
                "retrieval_refinement",
                "tool_use",
                "adaptive_threshold",
                "self_rag",
                "graph_rag",
                "crag",
                "speculative_rag",
                "map_reduce",
                "ragas",
                "compression",
                "multimodal",
                "raptor",
                "colbert",
                "active_learning",
            ] {
                store.set_setting(&format!("chat_pipeline.{feat}_enabled"), "false");
            }
        }
        "thai-recommended" => {
            // ── LLM: shared mode with Qwen3-14B ──
            store.set_setting("chat_pipeline.enabled", "true");
            store.set_setting("chat_pipeline.llm_mode", "shared");
            store.set_setting("chat_pipeline.llm", &ollama_llm("qwen3:14b", &url));
            // ── Agents ──
            store.set_setting("chat_pipeline.query_analyzer_enabled", "true");
            store.set_setting("chat_pipeline.query_rewriter_enabled", "true");
            store.set_setting("chat_pipeline.context_curator_enabled", "true");
            store.set_setting("chat_pipeline.quality_guard_enabled", "true");
            store.set_setting("chat_pipeline.language_adapter_enabled", "true");
            store.set_setting("chat_pipeline.orchestrator_enabled", "true");
            // ── Embedding ──
            store.set_setting("providers.embedding.kind", "ollama");
            store.set_setting("providers.embedding.model", "qwen3-embedding:8b");
            store.set_setting("providers.embedding.base_url", &url);
            store.set_setting("providers.embedding.dimensions", "4096");
            // ── Reranker: passthrough ──
            store.set_setting("providers.reranker.kind", "passthrough");
            // ── Tuning: balanced for 14B model ──
            store.set_setting("chat_pipeline.max_context_tokens", "4096");
            store.set_setting("chat_pipeline.agent_max_tokens", "2048");
            store.set_setting("chat_pipeline.quality_guard_threshold", "0.6");
            store.set_setting("chat_pipeline.quality_guard_max_retries", "1");
            store.set_setting("chat_pipeline.max_orchestrator_calls", "3");
            // ── Enable key features ──
            store.set_setting("chat_pipeline.conversation_memory_enabled", "true");
            store.set_setting("chat_pipeline.memory_max_summaries", "10");
            store.set_setting("chat_pipeline.memory_summary_max_tokens", "256");
            store.set_setting("chat_pipeline.active_learning_enabled", "true");
            store.set_setting("chat_pipeline.colbert_enabled", "true");
            store.set_setting("chat_pipeline.colbert_top_n", "5");
            // ── Disable heavy features ──
            for feat in &[
                "retrieval_refinement",
                "tool_use",
                "adaptive_threshold",
                "self_rag",
                "graph_rag",
                "crag",
                "speculative_rag",
                "map_reduce",
                "ragas",
                "compression",
                "multimodal",
                "raptor",
            ] {
                store.set_setting(&format!("chat_pipeline.{feat}_enabled"), "false");
            }
        }
        "thai-max" => {
            // ── LLM: per-agent mode with tiered models ──
            store.set_setting("chat_pipeline.enabled", "true");
            store.set_setting("chat_pipeline.llm_mode", "per-agent");
            // Heavy tasks: qwen3:32b
            store.set_setting(
                "chat_pipeline.response_generator_llm",
                &ollama_llm("qwen3:32b", &url),
            );
            store.set_setting(
                "chat_pipeline.quality_guard_llm",
                &ollama_llm("qwen3:32b", &url),
            );
            store.set_setting(
                "chat_pipeline.context_curator_llm",
                &ollama_llm("qwen3:32b", &url),
            );
            // Medium tasks: qwen3:14b
            store.set_setting(
                "chat_pipeline.orchestrator_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting(
                "chat_pipeline.graph_rag_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting("chat_pipeline.crag_llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting("chat_pipeline.raptor_llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting("chat_pipeline.colbert_llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting(
                "chat_pipeline.compression_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting("chat_pipeline.self_rag_llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting(
                "chat_pipeline.map_reduce_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting("chat_pipeline.memory_llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting("chat_pipeline.ragas_llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting("chat_pipeline.tool_use_llm", &ollama_llm("qwen3:14b", &url));
            // Light tasks: Chinda
            store.set_setting(
                "chat_pipeline.query_analyzer_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "chat_pipeline.query_rewriter_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "chat_pipeline.language_adapter_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            // Vision: Llama4 Scout
            store.set_setting(
                "chat_pipeline.multimodal_llm",
                &ollama_llm("llama4:scout", &url),
            );
            // ── Embedding ──
            store.set_setting("providers.embedding.kind", "ollama");
            store.set_setting("providers.embedding.model", "qwen3-embedding:8b");
            store.set_setting("providers.embedding.base_url", &url);
            store.set_setting("providers.embedding.dimensions", "4096");
            // ── Reranker: passthrough ──
            store.set_setting("providers.reranker.kind", "passthrough");
            // ── Tuning: aggressive for 32B model ──
            store.set_setting("chat_pipeline.max_context_tokens", "8192");
            store.set_setting("chat_pipeline.agent_max_tokens", "4096");
            store.set_setting("chat_pipeline.quality_guard_threshold", "0.7");
            store.set_setting("chat_pipeline.quality_guard_max_retries", "2");
            store.set_setting("chat_pipeline.max_orchestrator_calls", "5");
            // Feature params
            store.set_setting("chat_pipeline.memory_max_summaries", "20");
            store.set_setting("chat_pipeline.memory_summary_max_tokens", "512");
            store.set_setting("chat_pipeline.colbert_top_n", "10");
            store.set_setting("chat_pipeline.self_rag_threshold", "0.7");
            store.set_setting("chat_pipeline.graph_rag_max_entities", "50");
            store.set_setting("chat_pipeline.graph_rag_max_depth", "3");
            store.set_setting("chat_pipeline.crag_relevance_threshold", "0.5");
            store.set_setting("chat_pipeline.crag_max_web_results", "3");
            store.set_setting("chat_pipeline.compression_target_ratio", "0.5");
            store.set_setting("chat_pipeline.raptor_max_depth", "3");
            store.set_setting("chat_pipeline.raptor_group_size", "5");
            store.set_setting("chat_pipeline.map_reduce_max_chunks", "10");
            store.set_setting("chat_pipeline.ragas_sample_rate", "0.1");
            store.set_setting("chat_pipeline.multimodal_max_images", "5");
            store.set_setting("chat_pipeline.tool_use_max_calls", "5");
            store.set_setting("chat_pipeline.refinement_min_relevance", "0.3");
            store.set_setting("chat_pipeline.refinement_max_retries", "2");
            store.set_setting("chat_pipeline.adaptive_min_samples", "20");
            store.set_setting("chat_pipeline.feedback_decay_days", "30");
            store.set_setting("chat_pipeline.active_learning_min_interactions", "50");
            store.set_setting("chat_pipeline.active_learning_max_low_confidence", "10");
            // ── Enable all agents and features ──
            for agent in &[
                "query_analyzer",
                "query_rewriter",
                "context_curator",
                "quality_guard",
                "language_adapter",
                "orchestrator",
            ] {
                store.set_setting(&format!("chat_pipeline.{agent}_enabled"), "true");
            }
            for feat in &[
                "conversation_memory",
                "retrieval_refinement",
                "tool_use",
                "adaptive_threshold",
                "self_rag",
                "graph_rag",
                "crag",
                "compression",
                "multimodal",
                "raptor",
                "colbert",
                "active_learning",
                "map_reduce",
                "ragas",
            ] {
                store.set_setting(&format!("chat_pipeline.{feat}_enabled"), "true");
            }
            store.set_setting("chat_pipeline.speculative_rag_enabled", "false"); // experimental
        }
        "thai-doc-basic" => {
            // ── AI Preprocessing: all Chinda ──
            store.set_setting("ai_preprocessing.enabled", "true");
            store.set_setting("ai_preprocessing.auto_params", "true");
            store.set_setting(
                "ai_preprocessing.llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.analyzer_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.converter_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.quality_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.chunker_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.enricher_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.orchestrator_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting("ai_preprocessing.enricher_enabled", "true");
            store.set_setting("ai_preprocessing.orchestrator_enabled", "false");
            // ── Tuning: conservative for 4B ──
            store.set_setting("ai_preprocessing.agent_max_tokens", "1024");
            store.set_setting("ai_preprocessing.max_llm_input_chars", "4000");
            store.set_setting("ai_preprocessing.quality_threshold", "0.5");
            // ── Chunk params ──
            store.set_setting("document.max_chunk_size", "512");
            store.set_setting("document.chunk_overlap", "64");
            // ── Embedding ──
            store.set_setting("providers.embedding.kind", "ollama");
            store.set_setting("providers.embedding.model", "qwen3-embedding:0.6b");
            store.set_setting("providers.embedding.base_url", &url);
            store.set_setting("providers.embedding.dimensions", "1024");
        }
        "thai-doc-recommended" => {
            // ── AI Preprocessing: Qwen3-14B + Chinda enricher ──
            store.set_setting("ai_preprocessing.enabled", "true");
            store.set_setting("ai_preprocessing.auto_params", "true");
            store.set_setting("ai_preprocessing.llm", &ollama_llm("qwen3:14b", &url));
            store.set_setting(
                "ai_preprocessing.analyzer_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting(
                "ai_preprocessing.converter_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting(
                "ai_preprocessing.quality_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting(
                "ai_preprocessing.chunker_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting(
                "ai_preprocessing.enricher_llm",
                &ollama_llm("iapp/chinda-qwen3-4b", &url),
            );
            store.set_setting(
                "ai_preprocessing.orchestrator_llm",
                &ollama_llm("qwen3:14b", &url),
            );
            store.set_setting("ai_preprocessing.enricher_enabled", "true");
            store.set_setting("ai_preprocessing.orchestrator_enabled", "true");
            // ── Tuning: quality for 14B ──
            store.set_setting("ai_preprocessing.agent_max_tokens", "2048");
            store.set_setting("ai_preprocessing.max_llm_input_chars", "8000");
            store.set_setting("ai_preprocessing.quality_threshold", "0.7");
            store.set_setting("ai_preprocessing.max_orchestrator_calls", "3");
            store.set_setting("ai_preprocessing.auto_orchestrator_budget", "true");
            // ── Chunk params ──
            store.set_setting("document.max_chunk_size", "1024");
            store.set_setting("document.chunk_overlap", "128");
            // ── Embedding ──
            store.set_setting("providers.embedding.kind", "ollama");
            store.set_setting("providers.embedding.model", "qwen3-embedding:8b");
            store.set_setting("providers.embedding.base_url", &url);
            store.set_setting("providers.embedding.dimensions", "4096");
        }
        // ── Cloud Chat presets ──
        "cloud-basic" => {
            if api_key.is_empty() {
                return Err(ApiError(ThaiRagError::Validation(
                    "API key is required for cloud presets".into(),
                )));
            }
            let vk_id = ensure_vault_key(&api_key, "OpenAI (Preset)");
            let mini_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 Mini (Preset)", "gpt-4.1-mini");
            store.set_setting("chat_pipeline.enabled", "true");
            store.set_setting("chat_pipeline.llm_mode", "shared");
            store.set_setting("chat_pipeline.llm", &mini_cfg);
            // ── Agents ──
            store.set_setting("chat_pipeline.query_analyzer_enabled", "true");
            store.set_setting("chat_pipeline.query_rewriter_enabled", "true");
            store.set_setting("chat_pipeline.context_curator_enabled", "true");
            store.set_setting("chat_pipeline.quality_guard_enabled", "true");
            store.set_setting("chat_pipeline.language_adapter_enabled", "true");
            store.set_setting("chat_pipeline.orchestrator_enabled", "false");
            // ── Embedding: FastEmbed (local, no API key needed) ──
            store.set_setting("providers.embedding.kind", "fastembed");
            store.set_setting("providers.embedding.model", "BAAI/bge-small-en-v1.5");
            store.set_setting("providers.embedding.dimensions", "384");
            // ── Reranker: passthrough ──
            store.set_setting("providers.reranker.kind", "passthrough");
            // ── Tuning ──
            store.set_setting("chat_pipeline.max_context_tokens", "4096");
            store.set_setting("chat_pipeline.agent_max_tokens", "2048");
            store.set_setting("chat_pipeline.quality_guard_threshold", "0.6");
            store.set_setting("chat_pipeline.quality_guard_max_retries", "0");
            // ── Disable all advanced features ──
            for feat in &[
                "conversation_memory",
                "retrieval_refinement",
                "tool_use",
                "adaptive_threshold",
                "self_rag",
                "graph_rag",
                "crag",
                "speculative_rag",
                "map_reduce",
                "ragas",
                "compression",
                "multimodal",
                "raptor",
                "colbert",
                "active_learning",
            ] {
                store.set_setting(&format!("chat_pipeline.{feat}_enabled"), "false");
            }
        }
        "cloud-recommended" => {
            if api_key.is_empty() {
                return Err(ApiError(ThaiRagError::Validation(
                    "API key is required for cloud presets".into(),
                )));
            }
            let vk_id = ensure_vault_key(&api_key, "OpenAI (Preset)");
            let mini_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 Mini (Preset)", "gpt-4.1-mini");
            store.set_setting("chat_pipeline.enabled", "true");
            store.set_setting("chat_pipeline.llm_mode", "shared");
            store.set_setting("chat_pipeline.llm", &mini_cfg);
            // ── Agents ──
            store.set_setting("chat_pipeline.query_analyzer_enabled", "true");
            store.set_setting("chat_pipeline.query_rewriter_enabled", "true");
            store.set_setting("chat_pipeline.context_curator_enabled", "true");
            store.set_setting("chat_pipeline.quality_guard_enabled", "true");
            store.set_setting("chat_pipeline.language_adapter_enabled", "true");
            store.set_setting("chat_pipeline.orchestrator_enabled", "true");
            // ── Embedding: FastEmbed ──
            store.set_setting("providers.embedding.kind", "fastembed");
            store.set_setting("providers.embedding.model", "BAAI/bge-small-en-v1.5");
            store.set_setting("providers.embedding.dimensions", "384");
            // ── Reranker: passthrough ──
            store.set_setting("providers.reranker.kind", "passthrough");
            // ── Tuning ──
            store.set_setting("chat_pipeline.max_context_tokens", "4096");
            store.set_setting("chat_pipeline.agent_max_tokens", "2048");
            store.set_setting("chat_pipeline.quality_guard_threshold", "0.6");
            store.set_setting("chat_pipeline.quality_guard_max_retries", "1");
            store.set_setting("chat_pipeline.max_orchestrator_calls", "3");
            // ── Enable key features ──
            store.set_setting("chat_pipeline.conversation_memory_enabled", "true");
            store.set_setting("chat_pipeline.memory_max_summaries", "10");
            store.set_setting("chat_pipeline.memory_summary_max_tokens", "256");
            store.set_setting("chat_pipeline.active_learning_enabled", "true");
            store.set_setting("chat_pipeline.colbert_enabled", "true");
            store.set_setting("chat_pipeline.colbert_top_n", "5");
            store.set_setting("chat_pipeline.adaptive_threshold_enabled", "true");
            // ── Disable heavy features ──
            for feat in &[
                "retrieval_refinement",
                "tool_use",
                "self_rag",
                "graph_rag",
                "crag",
                "speculative_rag",
                "map_reduce",
                "ragas",
                "compression",
                "multimodal",
                "raptor",
            ] {
                store.set_setting(&format!("chat_pipeline.{feat}_enabled"), "false");
            }
        }
        "cloud-max" => {
            if api_key.is_empty() {
                return Err(ApiError(ThaiRagError::Validation(
                    "API key is required for cloud presets".into(),
                )));
            }
            let vk_id = ensure_vault_key(&api_key, "OpenAI (Preset)");
            let full_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 (Preset)", "gpt-4.1");
            let mini_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 Mini (Preset)", "gpt-4.1-mini");
            let nano_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 Nano (Preset)", "gpt-4.1-nano");
            store.set_setting("chat_pipeline.enabled", "true");
            store.set_setting("chat_pipeline.llm_mode", "per-agent");
            // Heavy tasks: gpt-4.1
            store.set_setting("chat_pipeline.response_generator_llm", &full_cfg);
            store.set_setting("chat_pipeline.quality_guard_llm", &full_cfg);
            store.set_setting("chat_pipeline.context_curator_llm", &full_cfg);
            // Medium tasks: gpt-4.1-mini
            store.set_setting("chat_pipeline.orchestrator_llm", &mini_cfg);
            store.set_setting("chat_pipeline.graph_rag_llm", &mini_cfg);
            store.set_setting("chat_pipeline.crag_llm", &mini_cfg);
            store.set_setting("chat_pipeline.raptor_llm", &mini_cfg);
            store.set_setting("chat_pipeline.colbert_llm", &mini_cfg);
            store.set_setting("chat_pipeline.compression_llm", &mini_cfg);
            store.set_setting("chat_pipeline.self_rag_llm", &mini_cfg);
            store.set_setting("chat_pipeline.map_reduce_llm", &mini_cfg);
            store.set_setting("chat_pipeline.memory_llm", &mini_cfg);
            store.set_setting("chat_pipeline.ragas_llm", &mini_cfg);
            store.set_setting("chat_pipeline.tool_use_llm", &mini_cfg);
            // Light tasks: gpt-4.1-nano
            store.set_setting("chat_pipeline.query_analyzer_llm", &nano_cfg);
            store.set_setting("chat_pipeline.query_rewriter_llm", &nano_cfg);
            store.set_setting("chat_pipeline.language_adapter_llm", &nano_cfg);
            // ── Embedding: FastEmbed ──
            store.set_setting("providers.embedding.kind", "fastembed");
            store.set_setting("providers.embedding.model", "BAAI/bge-small-en-v1.5");
            store.set_setting("providers.embedding.dimensions", "384");
            // ── Reranker: passthrough ──
            store.set_setting("providers.reranker.kind", "passthrough");
            // ── Tuning ──
            store.set_setting("chat_pipeline.max_context_tokens", "8192");
            store.set_setting("chat_pipeline.agent_max_tokens", "4096");
            store.set_setting("chat_pipeline.quality_guard_threshold", "0.7");
            store.set_setting("chat_pipeline.quality_guard_max_retries", "2");
            store.set_setting("chat_pipeline.max_orchestrator_calls", "5");
            // Feature params
            store.set_setting("chat_pipeline.memory_max_summaries", "20");
            store.set_setting("chat_pipeline.memory_summary_max_tokens", "512");
            store.set_setting("chat_pipeline.colbert_top_n", "10");
            store.set_setting("chat_pipeline.self_rag_threshold", "0.7");
            store.set_setting("chat_pipeline.graph_rag_max_entities", "50");
            store.set_setting("chat_pipeline.graph_rag_max_depth", "3");
            store.set_setting("chat_pipeline.crag_relevance_threshold", "0.5");
            store.set_setting("chat_pipeline.crag_max_web_results", "3");
            store.set_setting("chat_pipeline.compression_target_ratio", "0.5");
            store.set_setting("chat_pipeline.raptor_max_depth", "3");
            store.set_setting("chat_pipeline.raptor_group_size", "5");
            store.set_setting("chat_pipeline.map_reduce_max_chunks", "10");
            store.set_setting("chat_pipeline.ragas_sample_rate", "0.1");
            store.set_setting("chat_pipeline.tool_use_max_calls", "5");
            store.set_setting("chat_pipeline.refinement_min_relevance", "0.3");
            store.set_setting("chat_pipeline.refinement_max_retries", "2");
            store.set_setting("chat_pipeline.adaptive_min_samples", "20");
            store.set_setting("chat_pipeline.feedback_decay_days", "30");
            store.set_setting("chat_pipeline.active_learning_min_interactions", "50");
            store.set_setting("chat_pipeline.active_learning_max_low_confidence", "10");
            // ── Enable all agents and features ──
            for agent in &[
                "query_analyzer",
                "query_rewriter",
                "context_curator",
                "quality_guard",
                "language_adapter",
                "orchestrator",
            ] {
                store.set_setting(&format!("chat_pipeline.{agent}_enabled"), "true");
            }
            for feat in &[
                "conversation_memory",
                "retrieval_refinement",
                "tool_use",
                "adaptive_threshold",
                "self_rag",
                "graph_rag",
                "crag",
                "compression",
                "raptor",
                "colbert",
                "active_learning",
                "map_reduce",
                "ragas",
            ] {
                store.set_setting(&format!("chat_pipeline.{feat}_enabled"), "true");
            }
            store.set_setting("chat_pipeline.speculative_rag_enabled", "false");
            store.set_setting("chat_pipeline.multimodal_enabled", "false"); // no vision model in cloud preset
        }
        // ── Cloud Document presets ──
        "cloud-doc-basic" => {
            if api_key.is_empty() {
                return Err(ApiError(ThaiRagError::Validation(
                    "API key is required for cloud presets".into(),
                )));
            }
            let vk_id = ensure_vault_key(&api_key, "OpenAI (Preset)");
            let mini_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 Mini (Preset)", "gpt-4.1-mini");
            store.set_setting("ai_preprocessing.enabled", "true");
            store.set_setting("ai_preprocessing.auto_params", "true");
            store.set_setting("ai_preprocessing.llm", &mini_cfg);
            store.set_setting("ai_preprocessing.analyzer_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.converter_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.quality_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.chunker_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.enricher_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.orchestrator_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.enricher_enabled", "true");
            store.set_setting("ai_preprocessing.orchestrator_enabled", "false");
            store.set_setting("ai_preprocessing.agent_max_tokens", "1024");
            store.set_setting("ai_preprocessing.max_llm_input_chars", "4000");
            store.set_setting("ai_preprocessing.quality_threshold", "0.5");
            store.set_setting("document.max_chunk_size", "512");
            store.set_setting("document.chunk_overlap", "64");
            // ── Embedding: FastEmbed ──
            store.set_setting("providers.embedding.kind", "fastembed");
            store.set_setting("providers.embedding.model", "BAAI/bge-small-en-v1.5");
            store.set_setting("providers.embedding.dimensions", "384");
        }
        "cloud-doc-recommended" => {
            if api_key.is_empty() {
                return Err(ApiError(ThaiRagError::Validation(
                    "API key is required for cloud presets".into(),
                )));
            }
            let vk_id = ensure_vault_key(&api_key, "OpenAI (Preset)");
            let full_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 (Preset)", "gpt-4.1");
            let mini_cfg = ensure_llm_profile(&vk_id, "GPT-4.1 Mini (Preset)", "gpt-4.1-mini");
            store.set_setting("ai_preprocessing.enabled", "true");
            store.set_setting("ai_preprocessing.auto_params", "true");
            store.set_setting("ai_preprocessing.llm", &full_cfg);
            store.set_setting("ai_preprocessing.analyzer_llm", &full_cfg);
            store.set_setting("ai_preprocessing.converter_llm", &full_cfg);
            store.set_setting("ai_preprocessing.quality_llm", &full_cfg);
            store.set_setting("ai_preprocessing.chunker_llm", &full_cfg);
            store.set_setting("ai_preprocessing.enricher_llm", &mini_cfg);
            store.set_setting("ai_preprocessing.orchestrator_llm", &full_cfg);
            store.set_setting("ai_preprocessing.enricher_enabled", "true");
            store.set_setting("ai_preprocessing.orchestrator_enabled", "true");
            store.set_setting("ai_preprocessing.agent_max_tokens", "2048");
            store.set_setting("ai_preprocessing.max_llm_input_chars", "8000");
            store.set_setting("ai_preprocessing.quality_threshold", "0.7");
            store.set_setting("ai_preprocessing.max_orchestrator_calls", "3");
            store.set_setting("ai_preprocessing.auto_orchestrator_budget", "true");
            store.set_setting("document.max_chunk_size", "1024");
            store.set_setting("document.chunk_overlap", "128");
            // ── Embedding: FastEmbed ──
            store.set_setting("providers.embedding.kind", "fastembed");
            store.set_setting("providers.embedding.model", "BAAI/bge-small-en-v1.5");
            store.set_setting("providers.embedding.dimensions", "384");
        }
        _ => {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "Unknown preset: {}. Available: thai-basic, thai-recommended, thai-max, cloud-basic, cloud-recommended, cloud-max, thai-doc-basic, thai-doc-recommended, cloud-doc-basic, cloud-doc-recommended",
                req.preset_id
            ))));
        }
    }

    // Build updated ProvidersConfig from KV store overrides
    let mut pc = state.providers().providers_config.clone();

    // Apply embedding overrides from KV store
    if let Some(kind) = store.get_setting("providers.embedding.kind")
        && let Ok(k) = parse_embedding_kind(&kind)
    {
        pc.embedding.kind = k;
    }
    if let Some(model) = store.get_setting("providers.embedding.model") {
        pc.embedding.model = model;
    }
    if let Some(base_url) = store.get_setting("providers.embedding.base_url") {
        pc.embedding.base_url = base_url;
    }
    if let Some(dim) = store.get_setting("providers.embedding.dimensions")
        && let Ok(d) = dim.parse::<usize>()
    {
        pc.embedding.dimension = d;
    }

    // Apply reranker overrides from KV store
    if let Some(kind) = store.get_setting("providers.reranker.kind")
        && let Ok(k) = parse_reranker_kind(&kind)
    {
        pc.reranker.kind = k;
    }

    // Apply LLM override from chat pipeline preset (shared mode)
    if let Some(llm_json) = store.get_setting("chat_pipeline.llm")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&llm_json)
    {
        if let Some(kind) = v.get("kind").and_then(|k| k.as_str())
            && let Ok(k) = parse_llm_kind(kind)
        {
            pc.llm.kind = k;
        }
        if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
            pc.llm.model = model.to_string();
        }
        if let Some(base_url) = v.get("base_url").and_then(|u| u.as_str()) {
            pc.llm.base_url = base_url.to_string();
        }
        if let Some(api_key) = v.get("api_key").and_then(|k| k.as_str()) {
            pc.llm.api_key = api_key.to_string();
        }
    }

    // Persist the full provider_config blob so GET /providers returns updated values
    if let Ok(json) = serde_json::to_string(&pc) {
        store.set_setting("provider_config", &json);
    }

    // Hot-reload providers with updated config (read from DB, not file config)
    let eff_chat = get_effective_chat_pipeline(&state);
    let bundle =
        state.build_provider_bundle(&pc, &state.config.search, &state.config.document, &eff_chat);
    state.reload_providers(bundle);
    tracing::info!(preset = %req.preset_id, "Preset applied and providers hot-reloaded");

    Ok(Json(serde_json::json!({
        "preset": req.preset_id,
        "status": "applied",
        "message": format!("Preset '{}' applied successfully.", req.preset_id)
    })))
}

// ── Ollama Model Pull ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct OllamaPullRequest {
    pub model: String,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
}

#[derive(Serialize)]
pub struct OllamaPullResponse {
    pub model: String,
    pub status: String,
}

/// POST /api/km/settings/ollama/pull
/// Pulls an Ollama model. Returns immediately; pull happens in background.
pub async fn ollama_pull_model(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<OllamaPullRequest>,
) -> Result<Json<OllamaPullResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let model = req.model.clone();
    let url = req.ollama_url.clone();

    // Start pull in background
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let pull_url = format!("{}/api/pull", url.trim_end_matches('/'));
        match client
            .post(&pull_url)
            .json(&serde_json::json!({ "model": model, "stream": false }))
            .timeout(std::time::Duration::from_secs(3600))
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    tracing::info!(model = %model, "Ollama model pull completed");
                } else {
                    tracing::warn!(model = %model, status = %resp.status(), "Ollama model pull failed");
                }
            }
            Err(e) => {
                tracing::warn!(model = %model, error = %e, "Ollama model pull request failed");
            }
        }
    });

    Ok(Json(OllamaPullResponse {
        model: req.model,
        status: "pulling".into(),
    }))
}

/// GET /api/km/settings/ollama/models
/// Lists available Ollama models (proxy to Ollama API).
pub async fn list_ollama_models(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<ModelsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Get Ollama URL from current config
    let pc = state.providers().providers_config;
    let base_url = if pc.llm.base_url.is_empty() {
        "http://host.docker.internal:11435".to_string()
    } else {
        pc.llm.base_url.clone()
    };

    let kind = thairag_core::types::LlmKind::Ollama;
    Ok(Json(fetch_models_for_provider(&kind, &base_url, "").await))
}

// ── Public endpoint (no auth required) ──────────────────────────────

pub async fn list_enabled_providers(State(state): State<AppState>) -> Json<Vec<PublicIdpInfo>> {
    let providers = state.km_store.list_enabled_identity_providers();
    Json(
        providers
            .into_iter()
            .map(|p| PublicIdpInfo {
                id: p.id.0.to_string(),
                name: p.name,
                provider_type: p.provider_type,
            })
            .collect(),
    )
}

// ── Prompt Management ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PromptListItem {
    pub key: String,
    pub description: String,
    pub category: String,
    pub source: String,
    pub template: String,
}

/// GET /api/km/settings/prompts — list all prompt templates (super admin only).
pub async fn list_prompts(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<PromptListItem>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let entries = state.prompt_registry.list();
    Ok(Json(
        entries
            .into_iter()
            .map(|(key, entry)| PromptListItem {
                key,
                description: entry.description,
                category: entry.category,
                source: match entry.source {
                    PromptSource::Default => "default".to_string(),
                    PromptSource::Override => "override".to_string(),
                },
                template: entry.template,
            })
            .collect(),
    ))
}

/// GET /api/km/settings/prompts/{key} — get a single prompt template (super admin only).
pub async fn get_prompt(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(key): Path<String>,
) -> Result<Json<PromptListItem>, ApiError> {
    require_super_admin(&claims, &state)?;
    let entry = state
        .prompt_registry
        .get(&key)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound(format!("Prompt '{key}' not found"))))?;

    Ok(Json(PromptListItem {
        key,
        description: entry.description,
        category: entry.category,
        source: match entry.source {
            PromptSource::Default => "default".to_string(),
            PromptSource::Override => "override".to_string(),
        },
        template: entry.template,
    }))
}

#[derive(Deserialize)]
pub struct UpdatePromptRequest {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// PUT /api/km/settings/prompts/{key} — override a prompt template (super admin only).
pub async fn update_prompt(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(key): Path<String>,
    Json(body): Json<UpdatePromptRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;
    let category = key.split('.').next().unwrap_or("chat").to_string();

    // Determine description: use provided, or existing, or empty
    let description = body.description.unwrap_or_else(|| {
        state
            .prompt_registry
            .get(&key)
            .map(|e| e.description)
            .unwrap_or_default()
    });

    // Update in-memory registry
    state
        .prompt_registry
        .set(&key, body.template.clone(), description.clone(), category);

    // Persist to KV store
    state
        .km_store
        .set_setting(&format!("prompt.{key}"), &body.template);
    state
        .km_store
        .set_setting(&format!("prompt.{key}.description"), &description);

    // Update the prompt index for KV-only prompts
    update_prompt_index(&state);

    Ok(Json(serde_json::json!({ "status": "updated", "key": key })))
}

/// DELETE /api/km/settings/prompts/{key} — revert a prompt override to default (super admin only).
pub async fn delete_prompt_override(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;
    // Remove from KV store
    state.km_store.delete_setting(&format!("prompt.{key}"));
    state
        .km_store
        .delete_setting(&format!("prompt.{key}.description"));

    // Try to reload from file (if file version exists)
    let prompts_dir = std::path::Path::new("prompts");
    let _ = state.prompt_registry.load_from_dir(prompts_dir);

    // If no file version, remove entirely
    if state.prompt_registry.get(&key).is_none() {
        Ok(Json(serde_json::json!({ "status": "deleted", "key": key })))
    } else {
        // File version was reloaded
        state.prompt_registry.delete_override(&key);
        // Reload from directory to get the default back
        let _ = state.prompt_registry.load_from_dir(prompts_dir);
        Ok(Json(
            serde_json::json!({ "status": "reverted_to_default", "key": key }),
        ))
    }
}

/// Update the KV index of prompt keys (for prompts that exist only in KV store).
fn update_prompt_index(state: &AppState) {
    let keys: Vec<String> = state
        .prompt_registry
        .list()
        .into_iter()
        .filter(|(_, e)| e.source == PromptSource::Override)
        .map(|(k, _)| k)
        .collect();
    state.km_store.set_setting("prompt._index", &keys.join(","));
}

// ── Audit Log ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditLogParams {
    pub action: Option<String>,
    #[serde(default = "default_audit_limit")]
    pub limit: usize,
}

// ── Usage Stats ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct UsageStatsResponse {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub llm_kind: String,
    pub llm_model: String,
    pub embedding_kind: String,
    pub embedding_model: String,
    pub estimated_cost_usd: Option<f64>,
}

/// GET /api/km/settings/usage — return cumulative token usage + cost estimate.
pub async fn get_usage_stats(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<UsageStatsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let (prompt, completion) = state
        .km_store
        .get_setting("usage:tokens")
        .and_then(|v| serde_json::from_str::<(u64, u64)>(&v).ok())
        .unwrap_or((0, 0));

    let pc = &state.providers().providers_config;
    let llm_kind = format!("{:?}", pc.llm.kind).to_lowercase();
    let llm_model = pc.llm.model.clone();
    let embedding_kind = format!("{:?}", pc.embedding.kind).to_lowercase();
    let embedding_model = pc.embedding.model.clone();

    // Estimate cost based on known model pricing (per 1M tokens)
    let cost = estimate_cost(&llm_kind, &llm_model, prompt, completion);

    Ok(Json(UsageStatsResponse {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        llm_kind,
        llm_model,
        embedding_kind,
        embedding_model,
        estimated_cost_usd: cost,
    }))
}

/// Rough cost estimation based on known model pricing (USD per 1M tokens).
fn estimate_cost(kind: &str, model: &str, prompt: u64, completion: u64) -> Option<f64> {
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

fn default_audit_limit() -> usize {
    100
}

pub async fn get_audit_log(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<AuditLogParams>,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let limit = params.limit.min(1000);
    let entries = audit::get_audit_log(&state.km_store, params.action.as_deref(), limit);
    Ok(Json(entries))
}

// ── Vector Database Management ──────────────────────────────────────

/// GET /settings/vectordb/info — return stats about the vector store.
pub async fn get_vectordb_info(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    let p = state.providers();
    let stats = p.search_engine.vector_store_stats().await.map_err(|e| {
        tracing::warn!(error = %e, "Failed to get vector store stats");
        ApiError(e)
    });

    let cfg = &state.config.providers.vector_store;
    let stats = stats.unwrap_or_default();

    Ok(Json(serde_json::json!({
        "backend": kind_str(&cfg.kind),
        "url": cfg.url,
        "collection": cfg.collection,
        "isolation": format!("{:?}", cfg.isolation),
        "vector_count": stats.vector_count,
    })))
}

/// POST /settings/vectordb/clear — delete all vectors.
pub async fn clear_vectordb(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    let p = state.providers();
    p.search_engine
        .delete_all_vectors()
        .await
        .map_err(ApiError)?;

    audit::audit_log(
        &state.km_store,
        &claims.sub,
        audit::AuditAction::VectorDbCleared,
        "vectordb",
        true,
        Some("Cleared all vectors"),
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "message": "All vectors have been deleted. Documents will need to be re-processed."
    })))
}

// ── Config Snapshots ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    created_at: String,
    created_by: String,
    embedding_fingerprint: String,
    settings: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSnapshotRequest {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Serialize)]
pub struct SnapshotListItem {
    id: String,
    name: String,
    description: String,
    created_at: String,
    created_by: String,
    embedding_fingerprint: String,
    settings_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct RestoreQuery {
    #[serde(default)]
    force: bool,
    /// When true, restore all settings except embedding/vector-store config,
    /// preserving the current embeddings so no re-indexing is needed.
    #[serde(default)]
    skip_embedding: bool,
}

#[derive(Debug, Serialize)]
pub struct RestoreResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn get_embedding_fingerprint(state: &AppState) -> String {
    state
        .km_store
        .get_setting("_embedding_fingerprint")
        .unwrap_or_else(|| {
            let cfg = &state.providers().providers_config.embedding;
            format!("{:?}:{}:{}", cfg.kind, cfg.model, cfg.dimension)
        })
}

pub async fn create_snapshot(
    Extension(claims): Extension<AuthClaims>,
    State(state): State<AppState>,
    AppJson(req): AppJson<CreateSnapshotRequest>,
) -> Result<Json<ConfigSnapshot>, ApiError> {
    require_super_admin(&claims, &state)?;

    let id = Uuid::new_v4().to_string();
    let settings: std::collections::HashMap<String, String> =
        state.km_store.list_all_settings().into_iter().collect();

    let snapshot = ConfigSnapshot {
        id: id.clone(),
        name: req.name,
        description: req.description,
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: claims.sub.clone(),
        embedding_fingerprint: get_embedding_fingerprint(&state),
        settings,
    };

    let json = serde_json::to_string(&snapshot)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state.km_store.set_setting(&format!("snapshot.{id}"), &json);

    // Update snapshot index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting("_snapshot_index")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.push(id.clone());
    state
        .km_store
        .set_setting("_snapshot_index", &serde_json::to_string(&ids).unwrap());

    audit::audit_log(
        &state.km_store,
        &claims.sub,
        audit::AuditAction::SettingsChanged,
        "snapshot",
        true,
        Some(&format!("Created snapshot: {}", snapshot.name)),
    );

    Ok(Json(snapshot))
}

pub async fn list_snapshots(
    Extension(claims): Extension<AuthClaims>,
    State(state): State<AppState>,
) -> Result<Json<Vec<SnapshotListItem>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let index_str = state
        .km_store
        .get_setting("_snapshot_index")
        .unwrap_or_default();
    let ids: Vec<String> = if index_str.is_empty() {
        vec![]
    } else {
        serde_json::from_str(&index_str).unwrap_or_default()
    };

    let mut items = Vec::new();
    for id in &ids {
        if let Some(json) = state.km_store.get_setting(&format!("snapshot.{id}"))
            && let Ok(snap) = serde_json::from_str::<ConfigSnapshot>(&json)
        {
            items.push(SnapshotListItem {
                id: snap.id,
                name: snap.name,
                description: snap.description,
                created_at: snap.created_at,
                created_by: snap.created_by,
                embedding_fingerprint: snap.embedding_fingerprint,
                settings_count: snap.settings.len(),
            });
        }
    }

    // Sort by created_at descending
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(Json(items))
}

pub async fn restore_snapshot(
    Extension(claims): Extension<AuthClaims>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RestoreQuery>,
) -> Result<Json<RestoreResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let json = state
        .km_store
        .get_setting(&format!("snapshot.{id}"))
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Snapshot not found".into())))?;
    let snapshot: ConfigSnapshot =
        serde_json::from_str(&json).map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;

    // Check embedding fingerprint
    let current_fp = get_embedding_fingerprint(&state);
    let embedding_differs = snapshot.embedding_fingerprint != current_fp;

    // If embedding differs and neither force nor skip_embedding, return warning with options
    if embedding_differs && !query.force && !query.skip_embedding {
        return Ok(Json(RestoreResponse {
            status: "warning".into(),
            warning: Some(format!(
                "Embedding model differs: current={}, snapshot={}. \
                 You have two options:\n\
                 1. Restore without embedding changes (recommended) — keeps current vectors intact, no re-indexing needed. Use ?skip_embedding=true\n\
                 2. Restore everything — will clear all vectors and require re-indexing all documents. Use ?force=true",
                current_fp, snapshot.embedding_fingerprint
            )),
        }));
    }

    // When skip_embedding is set, preserve current embedding + vector store config
    let current_provider_config_json = if query.skip_embedding {
        state.km_store.get_setting("provider_config")
    } else {
        None
    };

    // Clear current non-snapshot, non-index settings
    let current_settings = state.km_store.list_all_settings();
    for (key, _) in &current_settings {
        if !key.starts_with("_snapshot_index") {
            state.km_store.delete_setting(key);
        }
    }

    // Write snapshot settings
    for (key, value) in &snapshot.settings {
        state.km_store.set_setting(key, value);
    }

    // If skipping embedding, merge back the current embedding/vector_store config
    if query.skip_embedding {
        if let Some(current_pc_json) = current_provider_config_json
            && let (Ok(mut snap_pc), Ok(current_pc)) = (
                serde_json::from_str::<serde_json::Value>(
                    snapshot
                        .settings
                        .get("provider_config")
                        .map(|s| s.as_str())
                        .unwrap_or("{}"),
                ),
                serde_json::from_str::<serde_json::Value>(&current_pc_json),
            )
        {
            // Keep current embedding + vector_store, take everything else from snapshot
            if let Some(obj) = snap_pc.as_object_mut() {
                if let Some(emb) = current_pc.get("embedding") {
                    obj.insert("embedding".to_string(), emb.clone());
                }
                if let Some(vs) = current_pc.get("vector_store") {
                    obj.insert("vector_store".to_string(), vs.clone());
                }
            }
            if let Ok(merged) = serde_json::to_string(&snap_pc) {
                state.km_store.set_setting("provider_config", &merged);
            }
        }
        // Restore the current embedding fingerprint
        state
            .km_store
            .set_setting("_embedding_fingerprint", &current_fp);
    } else if embedding_differs {
        // force=true with different embedding: clear vectors since embeddings are incompatible
        tracing::warn!(
            current = %current_fp,
            snapshot = %snapshot.embedding_fingerprint,
            "Restoring snapshot with different embedding — clearing vector store"
        );
        let _ = state.providers().search_engine.delete_all_vectors().await;
    }

    // Hot-reload providers
    let eff_chat = get_effective_chat_pipeline(&state);
    let pc = state
        .km_store
        .get_setting("provider_config")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| state.config.providers.clone());
    let bundle =
        state.build_provider_bundle(&pc, &state.config.search, &state.config.document, &eff_chat);
    state.reload_providers(bundle);

    audit::audit_log(
        &state.km_store,
        &claims.sub,
        audit::AuditAction::SettingsChanged,
        "snapshot",
        true,
        Some(&format!("Restored snapshot: {}", snapshot.name)),
    );

    Ok(Json(RestoreResponse {
        status: "restored".into(),
        warning: None,
    }))
}

pub async fn delete_snapshot(
    Extension(claims): Extension<AuthClaims>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    state.km_store.delete_setting(&format!("snapshot.{id}"));

    // Update index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting("_snapshot_index")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.retain(|i| i != &id);
    state
        .km_store
        .set_setting("_snapshot_index", &serde_json::to_string(&ids).unwrap());

    audit::audit_log(
        &state.km_store,
        &claims.sub,
        audit::AuditAction::SettingsChanged,
        "snapshot",
        true,
        Some(&format!("Deleted snapshot: {id}")),
    );

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

// ── Inference Logs ──────────────────────────────────────────────────

/// Query parameters for inference log endpoints.
#[derive(Debug, Deserialize, Default)]
pub struct InferenceLogFilterQuery {
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub status: Option<String>,
    pub llm_model: Option<String>,
    pub intent: Option<String>,
    pub response_id: Option<String>,
    pub session_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl InferenceLogFilterQuery {
    fn to_filter(&self, default_limit: usize) -> crate::store::InferenceLogFilter {
        crate::store::InferenceLogFilter {
            workspace_id: self.workspace_id.clone(),
            user_id: self.user_id.clone(),
            from_timestamp: self.from.clone(),
            to_timestamp: self.to.clone(),
            status: self.status.clone(),
            llm_model: self.llm_model.clone(),
            intent: self.intent.clone(),
            response_id: self.response_id.clone(),
            session_id: self.session_id.clone(),
            limit: self.limit.unwrap_or(default_limit),
            offset: self.offset.unwrap_or(0),
        }
    }
}

/// GET /api/km/settings/inference-logs
/// Query inference logs with filtering.
pub async fn list_inference_logs(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<InferenceLogFilterQuery>,
) -> Json<crate::store::InferenceLogListResponse> {
    let filter = q.to_filter(100);
    let entries = state.km_store.list_inference_logs(&filter);
    // Count with same filter but no limit/offset
    let count_filter = crate::store::InferenceLogFilter {
        limit: 0,
        offset: 0,
        ..filter
    };
    let total = state.km_store.count_inference_logs(&count_filter);
    Json(crate::store::InferenceLogListResponse { entries, total })
}

/// DELETE /api/km/settings/inference-logs
/// Delete inference logs matching the filter.
pub async fn delete_inference_logs(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<InferenceLogFilterQuery>,
) -> Json<serde_json::Value> {
    let filter = q.to_filter(0);
    let deleted = state.km_store.delete_inference_logs(&filter);
    Json(serde_json::json!({"ok": true, "deleted": deleted}))
}

/// GET /api/km/settings/inference-logs/export
/// Export inference logs (up to 50,000) as a flat JSON array.
pub async fn export_inference_logs(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<InferenceLogFilterQuery>,
) -> Json<Vec<crate::store::InferenceLogEntry>> {
    let filter = q.to_filter(50000);
    Json(state.km_store.list_inference_logs(&filter))
}

/// GET /api/km/settings/inference-analytics
/// Get aggregated inference statistics.
pub async fn get_inference_analytics(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<InferenceLogFilterQuery>,
) -> Json<crate::store::InferenceStats> {
    let filter = q.to_filter(10000);
    Json(state.km_store.get_inference_stats(&filter))
}

// ── Audit Log Export & Analytics ─────────────────────────────────────

/// GET /api/km/settings/audit-log/export
/// Export audit log entries with optional filtering.
pub async fn export_audit_logs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(filter): Query<crate::store::AuditLogFilter>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let entries = state.km_store.export_audit_logs(&filter);
    Ok(Json(entries))
}

/// GET /api/km/settings/audit-log/analytics
/// Get aggregated audit log analytics.
pub async fn get_audit_analytics(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(filter): Query<crate::store::AuditLogFilter>,
) -> Result<Json<crate::store::AuditAnalytics>, ApiError> {
    require_super_admin(&claims, &state)?;
    let analytics = state.km_store.get_audit_analytics(&filter);
    Ok(Json(analytics))
}
