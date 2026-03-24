use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{LlmProfileRow, VaultKeyRow};

use super::settings::require_super_admin;

// ── Request / Response types ────────────────────────────────────────

#[derive(Serialize)]
pub struct VaultKeyInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub key_masked: String,
    pub base_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct CreateVaultKeyRequest {
    pub name: String,
    pub provider: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
}

#[derive(Deserialize)]
pub struct UpdateVaultKeyRequest {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Serialize)]
pub struct LlmProfileInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub vault_key_id: Option<String>,
    pub vault_key_name: Option<String>,
    pub max_tokens: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct CreateLlmProfileRequest {
    pub name: String,
    pub kind: String,
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    pub vault_key_id: Option<String>,
    pub max_tokens: Option<u32>,
}

#[derive(Deserialize)]
pub struct UpdateLlmProfileRequest {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub vault_key_id: Option<String>,
    pub remove_vault_key: Option<bool>,
    pub max_tokens: Option<u32>,
}

#[derive(Serialize)]
pub struct TestResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn row_to_info(row: &VaultKeyRow) -> VaultKeyInfo {
    let masked = if row.key_prefix.is_empty() && row.key_suffix.is_empty() {
        "****".to_string()
    } else {
        format!("{}...{}", row.key_prefix, row.key_suffix)
    };
    VaultKeyInfo {
        id: row.id.clone(),
        name: row.name.clone(),
        provider: row.provider.clone(),
        key_masked: masked,
        base_url: row.base_url.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    }
}

fn profile_to_info(row: &LlmProfileRow, vault_key_name: Option<String>) -> LlmProfileInfo {
    LlmProfileInfo {
        id: row.id.clone(),
        name: row.name.clone(),
        kind: row.kind.clone(),
        model: row.model.clone(),
        base_url: row.base_url.clone(),
        vault_key_id: row.vault_key_id.clone(),
        vault_key_name,
        max_tokens: row.max_tokens,
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    }
}

const VALID_PROVIDERS: &[&str] = &["openai", "anthropic", "google", "cohere", "custom"];

// ── Vault Key endpoints ─────────────────────────────────────────────

pub async fn list_vault_keys(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<VaultKeyInfo>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let rows = state.km_store.list_vault_keys();
    let infos: Vec<VaultKeyInfo> = rows.iter().map(row_to_info).collect();
    Ok(Json(infos))
}

pub async fn create_vault_key(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<CreateVaultKeyRequest>,
) -> Result<(StatusCode, Json<VaultKeyInfo>), ApiError> {
    require_super_admin(&claims, &state)?;

    if req.name.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "name is required".into(),
        )));
    }
    if req.api_key.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "api_key is required".into(),
        )));
    }
    if !VALID_PROVIDERS.contains(&req.provider.as_str()) {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid provider '{}'. Must be one of: {}",
            req.provider,
            VALID_PROVIDERS.join(", ")
        ))));
    }

    let encrypted = state.vault.encrypt(&req.api_key);
    let key_prefix = if req.api_key.len() > 4 {
        req.api_key[..4].to_string()
    } else {
        String::new()
    };
    let key_suffix = if req.api_key.len() > 4 {
        req.api_key[req.api_key.len() - 4..].to_string()
    } else {
        String::new()
    };

    let now = chrono::Utc::now().to_rfc3339();
    let row = VaultKeyRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name.trim().to_string(),
        provider: req.provider,
        encrypted_key: encrypted,
        key_prefix,
        key_suffix,
        base_url: req.base_url,
        created_at: now.clone(),
        updated_at: now,
    };

    state.km_store.upsert_vault_key(&row);

    Ok((StatusCode::CREATED, Json(row_to_info(&row))))
}

pub async fn update_vault_key(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
    AppJson(req): AppJson<UpdateVaultKeyRequest>,
) -> Result<Json<VaultKeyInfo>, ApiError> {
    require_super_admin(&claims, &state)?;

    let mut row = state
        .km_store
        .get_vault_key(&id)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound(format!("Vault key {id} not found"))))?;

    if let Some(name) = req.name {
        if name.trim().is_empty() {
            return Err(ApiError(ThaiRagError::Validation(
                "name cannot be empty".into(),
            )));
        }
        row.name = name.trim().to_string();
    }
    if let Some(api_key) = req.api_key {
        if api_key.trim().is_empty() {
            return Err(ApiError(ThaiRagError::Validation(
                "api_key cannot be empty".into(),
            )));
        }
        row.encrypted_key = state.vault.encrypt(&api_key);
        row.key_prefix = if api_key.len() > 4 {
            api_key[..4].to_string()
        } else {
            String::new()
        };
        row.key_suffix = if api_key.len() > 4 {
            api_key[api_key.len() - 4..].to_string()
        } else {
            String::new()
        };
    }
    if let Some(base_url) = req.base_url {
        row.base_url = base_url;
    }

    row.updated_at = chrono::Utc::now().to_rfc3339();
    state.km_store.upsert_vault_key(&row);

    Ok(Json(row_to_info(&row)))
}

pub async fn delete_vault_key(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Check the key exists
    state
        .km_store
        .get_vault_key(&id)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound(format!("Vault key {id} not found"))))?;

    // Check if any profiles reference this key (warn but allow deletion)
    let referencing_profiles: Vec<String> = state
        .km_store
        .list_llm_profiles()
        .iter()
        .filter(|p| p.vault_key_id.as_deref() == Some(&id))
        .map(|p| p.name.clone())
        .collect();

    state.km_store.delete_vault_key(&id);

    let mut resp = serde_json::json!({ "deleted": true });
    if !referencing_profiles.is_empty() {
        resp["warning"] = serde_json::json!(format!(
            "The following LLM profiles referenced this key and may need updating: {}",
            referencing_profiles.join(", ")
        ));
    }

    Ok(Json(resp))
}

pub async fn test_vault_key(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<Json<TestResult>, ApiError> {
    require_super_admin(&claims, &state)?;

    let row = state
        .km_store
        .get_vault_key(&id)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound(format!("Vault key {id} not found"))))?;

    // Decrypt and validate the key is not empty
    match state.vault.decrypt(&row.encrypted_key) {
        Ok(plaintext) if !plaintext.trim().is_empty() => Ok(Json(TestResult {
            status: "ok".to_string(),
            message: Some(format!(
                "Key decrypted successfully for provider '{}'",
                row.provider
            )),
        })),
        Ok(_) => Ok(Json(TestResult {
            status: "error".to_string(),
            message: Some("Decrypted key is empty".to_string()),
        })),
        Err(e) => Ok(Json(TestResult {
            status: "error".to_string(),
            message: Some(format!("Failed to decrypt key: {e}")),
        })),
    }
}

// ── LLM Profile endpoints ───────────────────────────────────────────

pub async fn list_llm_profiles(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<LlmProfileInfo>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let profiles = state.km_store.list_llm_profiles();
    let vault_keys = state.km_store.list_vault_keys();

    let infos: Vec<LlmProfileInfo> = profiles
        .iter()
        .map(|p| {
            let key_name = p.vault_key_id.as_ref().and_then(|kid| {
                vault_keys
                    .iter()
                    .find(|k| k.id == *kid)
                    .map(|k| k.name.clone())
            });
            profile_to_info(p, key_name)
        })
        .collect();

    Ok(Json(infos))
}

pub async fn create_llm_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<CreateLlmProfileRequest>,
) -> Result<(StatusCode, Json<LlmProfileInfo>), ApiError> {
    require_super_admin(&claims, &state)?;

    if req.name.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "name is required".into(),
        )));
    }
    if req.kind.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "kind is required".into(),
        )));
    }
    if req.model.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "model is required".into(),
        )));
    }

    // Validate vault_key_id exists if provided
    let vault_key_name = if let Some(ref kid) = req.vault_key_id {
        let key = state.km_store.get_vault_key(kid).ok_or_else(|| {
            ApiError(ThaiRagError::Validation(format!(
                "Vault key '{kid}' not found"
            )))
        })?;
        Some(key.name)
    } else {
        None
    };

    let now = chrono::Utc::now().to_rfc3339();
    let row = LlmProfileRow {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name.trim().to_string(),
        kind: req.kind,
        model: req.model,
        base_url: req.base_url,
        vault_key_id: req.vault_key_id,
        max_tokens: req.max_tokens,
        created_at: now.clone(),
        updated_at: now,
    };

    state.km_store.upsert_llm_profile(&row);

    Ok((
        StatusCode::CREATED,
        Json(profile_to_info(&row, vault_key_name)),
    ))
}

pub async fn update_llm_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
    AppJson(req): AppJson<UpdateLlmProfileRequest>,
) -> Result<Json<LlmProfileInfo>, ApiError> {
    require_super_admin(&claims, &state)?;

    let mut row = state.km_store.get_llm_profile(&id).ok_or_else(|| {
        ApiError(ThaiRagError::NotFound(format!(
            "LLM profile {id} not found"
        )))
    })?;

    if let Some(name) = req.name {
        if name.trim().is_empty() {
            return Err(ApiError(ThaiRagError::Validation(
                "name cannot be empty".into(),
            )));
        }
        row.name = name.trim().to_string();
    }
    if let Some(kind) = req.kind {
        row.kind = kind;
    }
    if let Some(model) = req.model {
        row.model = model;
    }
    if let Some(base_url) = req.base_url {
        row.base_url = base_url;
    }
    if let Some(max_tokens) = req.max_tokens {
        row.max_tokens = Some(max_tokens);
    }

    // Handle vault key assignment / removal
    if req.remove_vault_key == Some(true) {
        row.vault_key_id = None;
    } else if let Some(ref kid) = req.vault_key_id {
        // Validate the vault key exists
        state.km_store.get_vault_key(kid).ok_or_else(|| {
            ApiError(ThaiRagError::Validation(format!(
                "Vault key '{kid}' not found"
            )))
        })?;
        row.vault_key_id = Some(kid.clone());
    }

    row.updated_at = chrono::Utc::now().to_rfc3339();
    state.km_store.upsert_llm_profile(&row);

    // Resolve vault key name for response
    let vault_key_name = row
        .vault_key_id
        .as_ref()
        .and_then(|kid| state.km_store.get_vault_key(kid).map(|k| k.name.clone()));

    Ok(Json(profile_to_info(&row, vault_key_name)))
}

pub async fn delete_llm_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    state.km_store.get_llm_profile(&id).ok_or_else(|| {
        ApiError(ThaiRagError::NotFound(format!(
            "LLM profile {id} not found"
        )))
    })?;

    state.km_store.delete_llm_profile(&id);

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Router ──────────────────────────────────────────────────────────

pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post, put};

    axum::Router::new()
        .route("/keys", get(list_vault_keys).post(create_vault_key))
        .route("/keys/{id}", put(update_vault_key).delete(delete_vault_key))
        .route("/keys/{id}/test", post(test_vault_key))
        .route("/profiles", get(list_llm_profiles).post(create_llm_profile))
        .route(
            "/profiles/{id}",
            put(update_llm_profile).delete(delete_llm_profile),
        )
}
