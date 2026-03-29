use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{CustomRole, RolePermission};
use thairag_core::ThaiRagError;

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: String,
    pub permissions: Vec<RolePermission>,
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub name: String,
    pub description: String,
    pub permissions: Vec<RolePermission>,
}

#[derive(Serialize)]
pub struct ListResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: usize,
}

// ── Validation ────────────────────────────────────────────────────────

fn validate_role_request(name: &str, permissions: &[RolePermission]) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "role name must not be empty".into(),
        )));
    }
    if name.len() > 100 {
        return Err(ApiError(ThaiRagError::Validation(
            "role name must not exceed 100 characters".into(),
        )));
    }
    if permissions.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "permissions must not be empty".into(),
        )));
    }
    Ok(())
}

// ── Handlers ─────────────────────────────────────────────────────────

pub async fn create_role(
    State(state): State<AppState>,
    AppJson(body): AppJson<CreateRoleRequest>,
) -> Result<(StatusCode, Json<CustomRole>), ApiError> {
    validate_role_request(&body.name, &body.permissions)?;
    let role = CustomRole {
        id: uuid::Uuid::new_v4().to_string(),
        name: body.name,
        description: body.description,
        permissions: body.permissions,
        is_system: false,
        created_at: Utc::now().to_rfc3339(),
    };
    let created = state.km_store.insert_custom_role(&role)?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_roles(State(state): State<AppState>) -> Json<ListResponse<CustomRole>> {
    let data = state.km_store.list_custom_roles();
    let total = data.len();
    Json(ListResponse { data, total })
}

pub async fn get_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CustomRole>, ApiError> {
    let role = state.km_store.get_custom_role(&id)?;
    Ok(Json(role))
}

pub async fn update_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
    AppJson(body): AppJson<UpdateRoleRequest>,
) -> Result<Json<CustomRole>, ApiError> {
    validate_role_request(&body.name, &body.permissions)?;
    let existing = state.km_store.get_custom_role(&id)?;
    let updated = CustomRole {
        id: existing.id,
        name: body.name,
        description: body.description,
        permissions: body.permissions,
        is_system: existing.is_system,
        created_at: existing.created_at,
    };
    state.km_store.update_custom_role(&updated)?;
    Ok(Json(updated))
}

pub async fn delete_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.km_store.delete_custom_role(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
