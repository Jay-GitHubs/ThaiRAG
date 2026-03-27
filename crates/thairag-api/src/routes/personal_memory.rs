use axum::Extension;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use thairag_auth::AuthClaims;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::store::PersonalMemoryRow;

use super::settings::require_super_admin;

// ── Query params ──────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct LimitParam {
    pub limit: Option<usize>,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/km/users/{user_id}/memories
pub async fn list_memories(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(user_id): Path<String>,
    Query(params): Query<LimitParam>,
) -> Result<Json<Vec<PersonalMemoryRow>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let limit = params.limit.unwrap_or(50).min(200);
    let memories = state.km_store.list_personal_memories(&user_id, limit);
    Ok(Json(memories))
}

/// DELETE /api/km/users/{user_id}/memories/{memory_id}
pub async fn delete_memory(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((user_id, memory_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;
    let _ = user_id; // validated by path
    state
        .km_store
        .delete_personal_memory(&memory_id)
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/km/users/{user_id}/memories
pub async fn delete_all_memories(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(user_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;
    state
        .km_store
        .delete_all_personal_memories(&user_id)
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
