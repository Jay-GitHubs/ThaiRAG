use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use thairag_core::models::User;
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::ApiError;

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
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    if body.email.trim().is_empty() || body.name.trim().is_empty() || body.password.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "All fields (email, name, password) are required and must be non-empty".into(),
        )));
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(body.password.as_bytes(), &salt)
        .map_err(|e| ApiError(ThaiRagError::Internal(format!("Password hashing failed: {e}"))))?
        .to_string();

    let user = state
        .km_store
        .insert_user(body.email, body.name, password_hash)?;

    Ok((StatusCode::CREATED, Json(user)))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    if body.email.trim().is_empty() || body.password.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "Email and password are required".into(),
        )));
    }

    let record = state
        .km_store
        .get_user_by_email(&body.email)
        .map_err(|_| ApiError(ThaiRagError::Auth("Invalid email or password".into())))?;

    let parsed_hash = PasswordHash::new(&record.password_hash)
        .map_err(|e| ApiError(ThaiRagError::Internal(format!("Password hash parse error: {e}"))))?;

    Argon2::default()
        .verify_password(body.password.as_bytes(), &parsed_hash)
        .map_err(|_| ApiError(ThaiRagError::Auth("Invalid email or password".into())))?;

    let jwt = state
        .jwt
        .as_ref()
        .ok_or_else(|| ApiError(ThaiRagError::Auth("Auth is not enabled".into())))?;

    let token = jwt
        .encode(&record.user.id.0.to_string(), &record.user.email)
        .map_err(ApiError::from)?;

    Ok(Json(LoginResponse {
        token,
        user: record.user,
    }))
}
