use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{Tenant, TenantQuota, TenantUsage};
use thairag_core::types::OrgId;

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub plan: String,
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub name: String,
    pub plan: String,
}

#[derive(Deserialize)]
pub struct AssignOrgRequest {
    pub org_id: uuid::Uuid,
}

#[derive(Serialize)]
pub struct ListResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: usize,
}

// ── Handlers ─────────────────────────────────────────────────────────

pub async fn create_tenant(
    State(state): State<AppState>,
    AppJson(body): AppJson<CreateTenantRequest>,
) -> Result<(StatusCode, Json<Tenant>), ApiError> {
    let tenant = state.km_store.insert_tenant(body.name, body.plan)?;
    Ok((StatusCode::CREATED, Json(tenant)))
}

pub async fn list_tenants(State(state): State<AppState>) -> Json<ListResponse<Tenant>> {
    let data = state.km_store.list_tenants();
    let total = data.len();
    Json(ListResponse { data, total })
}

pub async fn get_tenant(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Tenant>, ApiError> {
    let tenant = state.km_store.get_tenant(&id)?;
    Ok(Json(tenant))
}

pub async fn update_tenant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    AppJson(body): AppJson<UpdateTenantRequest>,
) -> Result<Json<Tenant>, ApiError> {
    let tenant = state.km_store.update_tenant(&id, body.name, body.plan)?;
    Ok(Json(tenant))
}

pub async fn delete_tenant(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.km_store.delete_tenant(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_quota(State(state): State<AppState>, Path(id): Path<String>) -> Json<TenantQuota> {
    let quota = state.km_store.get_tenant_quota(&id);
    Json(quota)
}

pub async fn set_quota(
    State(state): State<AppState>,
    Path(id): Path<String>,
    AppJson(quota): AppJson<TenantQuota>,
) -> Result<Json<TenantQuota>, ApiError> {
    state.km_store.set_tenant_quota(&id, &quota)?;
    Ok(Json(quota))
}

pub async fn get_usage(State(state): State<AppState>, Path(id): Path<String>) -> Json<TenantUsage> {
    let usage = state.km_store.get_tenant_usage(&id);
    Json(usage)
}

pub async fn assign_org(
    State(state): State<AppState>,
    Path(id): Path<String>,
    AppJson(body): AppJson<AssignOrgRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .km_store
        .assign_org_to_tenant(OrgId(body.org_id), &id)?;
    Ok(StatusCode::NO_CONTENT)
}
