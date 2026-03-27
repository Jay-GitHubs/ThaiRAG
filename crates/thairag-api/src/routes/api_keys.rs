use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::ApiKeyId;

use crate::app_state::AppState;
use crate::audit::{AuditAction, audit_log};
use crate::error::{ApiError, AppJson};

// ── Request / Response types ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    /// Role to assign to this key (e.g., "viewer", "editor", "admin").
    /// Defaults to the creating user's own role if omitted.
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Serialize)]
pub struct CreateApiKeyResponse {
    pub id: Uuid,
    pub name: String,
    /// The raw API key — returned only once at creation time.
    pub key: String,
    pub key_prefix: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ApiKeyInfo {
    pub id: Uuid,
    pub name: String,
    /// Prefix shown for identification (e.g. "trag_abc1...").
    pub key_prefix: String,
    pub role: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub is_active: bool,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// POST /api/auth/api-keys — create a new API key.
/// Returns the raw key exactly once.
pub async fn create_api_key(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    AppJson(body): AppJson<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "API key name is required".into(),
        )));
    }

    // Generate a random 32-byte key, base64url-encoded, prefixed with "trag_"
    let random_bytes: [u8; 32] = {
        // Use two UUIDs (16 bytes each) to get 32 random bytes
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        let mut buf = [0u8; 32];
        buf[..16].copy_from_slice(u1.as_bytes());
        buf[16..].copy_from_slice(u2.as_bytes());
        buf
    };
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        random_bytes,
    );
    let raw_key = format!("trag_{encoded}");

    // Hash the key with SHA-256 for storage
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    // Key prefix for display (first 12 chars of the raw key)
    let key_prefix = format!("{}...", &raw_key[..12.min(raw_key.len())]);

    // Determine the role: use requested role or fall back to user's role
    let user = state
        .km_store
        .get_user(thairag_core::types::UserId(claims.sub.parse().map_err(
            |_| ApiError(ThaiRagError::Auth("Invalid user ID in token".into())),
        )?))
        .map_err(|_| ApiError(ThaiRagError::Auth("User not found".into())))?;

    let role = body.role.unwrap_or_else(|| user.role.clone());

    let api_key = state.km_store.create_api_key(
        thairag_core::types::UserId(claims.sub.parse().unwrap()),
        body.name.clone(),
        key_hash,
        key_prefix.clone(),
        role.clone(),
    )?;

    audit_log(
        &state.km_store,
        &claims.sub,
        AuditAction::ApiKeyCreated,
        &body.name,
        true,
        Some(&format!("role={role}")),
    );

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            id: api_key.id.0,
            name: api_key.name,
            key: raw_key,
            key_prefix,
            role: api_key.role,
            created_at: api_key.created_at,
        }),
    ))
}

/// GET /api/auth/api-keys — list the current user's API keys.
pub async fn list_api_keys(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    let user_id = thairag_core::types::UserId(
        claims
            .sub
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Auth("Invalid user ID in token".into())))?,
    );

    let keys = state.km_store.list_api_keys(user_id);
    let infos: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            id: k.id.0,
            name: k.name,
            key_prefix: k.key_prefix,
            role: k.role,
            created_at: k.created_at,
            last_used_at: k.last_used_at,
            is_active: k.is_active,
        })
        .collect();

    Ok(Json(infos))
}

/// DELETE /api/auth/api-keys/:key_id — revoke an API key.
pub async fn revoke_api_key(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Verify the key belongs to the requesting user (or they're super_admin)
    let user_id = thairag_core::types::UserId(
        claims
            .sub
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Auth("Invalid user ID in token".into())))?,
    );

    let user_keys = state.km_store.list_api_keys(user_id);
    let user = state.km_store.get_user(user_id).ok();
    let is_super = user.map(|u| u.is_super_admin).unwrap_or(false);

    if !is_super && !user_keys.iter().any(|k| k.id.0 == key_id) {
        return Err(ApiError(ThaiRagError::Authorization(
            "You can only revoke your own API keys".into(),
        )));
    }

    state.km_store.revoke_api_key(ApiKeyId(key_id))?;

    audit_log(
        &state.km_store,
        &claims.sub,
        AuditAction::ApiKeyRevoked,
        &key_id.to_string(),
        true,
        None,
    );

    Ok(StatusCode::NO_CONTENT)
}
