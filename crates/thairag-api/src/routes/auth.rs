use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use thairag_core::ThaiRagError;
use thairag_core::models::User;
use thairag_core::types::IdpId;

use crate::app_state::AppState;
use crate::audit::{AuditAction, audit_log};
use crate::error::{ApiError, AppJson};
use crate::oidc::{
    OidcPendingAuth, OidcProviderConfig, build_authorize_url, exchange_code_for_user, resolve_role,
};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub name: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
    /// CSRF token for use with cookie-based auth flows.
    /// Include as `X-CSRF-Token` header on state-changing requests.
    pub csrf_token: String,
}

pub async fn register(
    State(state): State<AppState>,
    AppJson(body): AppJson<RegisterRequest>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    if body.email.trim().is_empty() || body.name.trim().is_empty() || body.password.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "All fields (email, name, password) are required and must be non-empty".into(),
        )));
    }

    // Password policy enforcement
    let min_len = state.config.auth.password_min_length;
    if body.password.len() < min_len {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Password must be at least {min_len} characters"
        ))));
    }
    let has_upper = body.password.chars().any(|c| c.is_uppercase());
    let has_lower = body.password.chars().any(|c| c.is_lowercase());
    let has_digit = body.password.chars().any(|c| c.is_ascii_digit());
    if !has_upper || !has_lower || !has_digit {
        return Err(ApiError(ThaiRagError::Validation(
            "Password must contain at least one uppercase letter, one lowercase letter, and one digit".into(),
        )));
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(body.password.as_bytes(), &salt)
        .map_err(|e| {
            ApiError(ThaiRagError::Internal(format!(
                "Password hashing failed: {e}"
            )))
        })?
        .to_string();

    // First user auto-promoted to super_admin (bootstrap mechanism)
    let is_first_user = state.km_store.list_users().is_empty();

    let user = if is_first_user {
        state.km_store.upsert_user_by_email(
            body.email,
            body.name,
            password_hash,
            true,
            "super_admin".into(),
        )?
    } else {
        state
            .km_store
            .insert_user(body.email, body.name, password_hash)?
    };

    audit_log(
        &state.km_store,
        &user.id.0.to_string(),
        AuditAction::Register,
        &user.email,
        true,
        if is_first_user {
            Some("first user — promoted to super_admin")
        } else {
            None
        },
    );

    Ok((StatusCode::CREATED, Json(user)))
}

// ── OIDC / OAuth2 Authorize Redirect ────────────────────────────

pub async fn oauth_authorize(
    State(state): State<AppState>,
    Path(provider_id): Path<Uuid>,
) -> Result<Redirect, ApiError> {
    let idp = state.km_store.get_identity_provider(IdpId(provider_id))?;

    if !idp.enabled {
        return Err(ApiError(ThaiRagError::Validation(
            "Identity provider is disabled".into(),
        )));
    }

    match idp.provider_type.as_str() {
        "oidc" | "oauth2" => {}
        other => {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "Provider type '{other}' does not support OAuth redirect flow"
            ))));
        }
    }

    let oidc_config = OidcProviderConfig::from_json(&idp.config)?;
    let auth = build_authorize_url(&oidc_config).await?;

    state.oidc_state_cache.store(
        auth.state.clone(),
        OidcPendingAuth {
            provider_id: provider_id.to_string(),
            pkce_verifier: auth.pkce_verifier,
            nonce: auth.nonce,
            created_at: Instant::now(),
        },
    );

    tracing::info!(
        provider = %idp.name,
        "OIDC authorize redirect"
    );

    Ok(Redirect::temporary(&auth.authorize_url))
}

// ── OIDC / OAuth2 Callback ──────────────────────────────────────

#[derive(Deserialize)]
pub struct OAuthCallbackParams {
    pub code: String,
    pub state: String,
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<OAuthCallbackParams>,
) -> Result<Redirect, ApiError> {
    let pending = state.oidc_state_cache.take(&params.state).ok_or_else(|| {
        ApiError(ThaiRagError::Auth(
            "Invalid or expired OAuth state parameter".into(),
        ))
    })?;

    // Check state age (10 min max)
    if pending.created_at.elapsed() > std::time::Duration::from_secs(600) {
        return Err(ApiError(ThaiRagError::Auth(
            "OAuth state has expired".into(),
        )));
    }

    let provider_id: Uuid = pending.provider_id.parse().map_err(|_| {
        ApiError(ThaiRagError::Internal(
            "Invalid provider_id in state".into(),
        ))
    })?;
    let idp = state.km_store.get_identity_provider(IdpId(provider_id))?;
    let oidc_config = OidcProviderConfig::from_json(&idp.config)?;

    let user_info = exchange_code_for_user(
        &oidc_config,
        &params.code,
        &pending.pkce_verifier,
        &pending.nonce,
    )
    .await?;

    // Resolve ThaiRAG role from Keycloak roles using the IdP's role_mapping
    let mapped_role = resolve_role(&user_info.roles, &oidc_config.role_mapping);
    let is_super = mapped_role == "super_admin";

    tracing::info!(
        provider = %idp.name,
        email = %user_info.email,
        external_id = %user_info.external_id,
        keycloak_roles = ?user_info.roles,
        mapped_role = %mapped_role,
        "OIDC user authenticated"
    );

    // Upsert user with mapped role
    let user = state.km_store.upsert_user_by_email(
        user_info.email,
        user_info.name,
        String::new(),
        is_super,
        mapped_role,
    )?;

    let jwt = state
        .jwt
        .as_ref()
        .ok_or_else(|| ApiError(ThaiRagError::Auth("Auth is not enabled".into())))?;

    let token = jwt
        .encode(&user.id.0.to_string(), &user.email)
        .map_err(ApiError::from)?;

    // Redirect to frontend with token as a fragment parameter
    // The frontend will pick this up and store it
    let redirect_url = format!(
        "/login#token={}&user={}",
        token,
        urlencoding::encode(&serde_json::to_string(&user).unwrap_or_default())
    );

    Ok(Redirect::temporary(&redirect_url))
}

// ── LDAP login (still stubbed) ──────────────────────────────────

pub async fn ldap_login() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": { "message": "LDAP authentication is not yet implemented", "type": "not_implemented" }
        })),
    )
}

pub async fn login(
    State(state): State<AppState>,
    AppJson(body): AppJson<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    if body.email.trim().is_empty() || body.password.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "Email and password are required".into(),
        )));
    }

    // Brute-force protection: check if account is locked
    if state.login_tracker.is_locked(&body.email) {
        let remaining = state.login_tracker.lockout_remaining_secs(&body.email);
        return Err(ApiError(ThaiRagError::Auth(format!(
            "Account temporarily locked due to too many failed attempts. Try again in {remaining} seconds"
        ))));
    }

    let record = state.km_store.get_user_by_email(&body.email).map_err(|_| {
        state.login_tracker.record_failure(&body.email);
        audit_log(
            &state.km_store,
            "unknown",
            AuditAction::LoginFailed,
            &body.email,
            false,
            Some("user not found"),
        );
        ApiError(ThaiRagError::Auth("Invalid email or password".into()))
    })?;

    // Check if user is disabled
    if record.user.disabled {
        audit_log(
            &state.km_store,
            &record.user.id.0.to_string(),
            AuditAction::LoginFailed,
            &body.email,
            false,
            Some("account disabled"),
        );
        return Err(ApiError(ThaiRagError::Auth(
            "This account has been disabled. Contact your administrator.".into(),
        )));
    }

    let parsed_hash = PasswordHash::new(&record.password_hash).map_err(|e| {
        ApiError(ThaiRagError::Internal(format!(
            "Password hash parse error: {e}"
        )))
    })?;

    if Argon2::default()
        .verify_password(body.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        state.login_tracker.record_failure(&body.email);
        audit_log(
            &state.km_store,
            &record.user.id.0.to_string(),
            AuditAction::LoginFailed,
            &body.email,
            false,
            Some("invalid password"),
        );
        return Err(ApiError(ThaiRagError::Auth(
            "Invalid email or password".into(),
        )));
    }

    // Success — clear any tracked failures
    state.login_tracker.record_success(&body.email);
    audit_log(
        &state.km_store,
        &record.user.id.0.to_string(),
        AuditAction::Login,
        &body.email,
        true,
        None,
    );

    let jwt = state
        .jwt
        .as_ref()
        .ok_or_else(|| ApiError(ThaiRagError::Auth("Auth is not enabled".into())))?;

    let token = jwt
        .encode(&record.user.id.0.to_string(), &record.user.email)
        .map_err(ApiError::from)?;

    let csrf_token = Uuid::new_v4().to_string();

    Ok(Json(LoginResponse {
        token,
        user: record.user,
        csrf_token,
    }))
}
