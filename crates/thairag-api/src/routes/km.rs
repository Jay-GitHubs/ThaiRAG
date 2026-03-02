use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_core::models::{Department, Organization, Workspace};
use thairag_core::types::{DeptId, OrgId, WorkspaceId};

use crate::app_state::AppState;
use crate::error::ApiError;

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateOrgRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct CreateDeptRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
}

#[derive(Serialize)]
pub struct ListResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: usize,
}

// ── Organization handlers ───────────────────────────────────────────

pub async fn create_org(
    State(state): State<AppState>,
    Json(body): Json<CreateOrgRequest>,
) -> Result<(StatusCode, Json<Organization>), ApiError> {
    let org = state.km_store.insert_org(body.name)?;
    Ok((StatusCode::CREATED, Json(org)))
}

pub async fn list_orgs(
    State(state): State<AppState>,
) -> Json<ListResponse<Organization>> {
    let orgs = state.km_store.list_orgs();
    let total = orgs.len();
    Json(ListResponse { data: orgs, total })
}

pub async fn get_org(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Organization>, ApiError> {
    let org = state.km_store.get_org(OrgId(org_id))?;
    Ok(Json(org))
}

pub async fn delete_org(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let doc_ids = state.km_store.cascade_delete_org(OrgId(org_id))?;
    for doc_id in doc_ids {
        let _ = state.search_engine.delete_doc(doc_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Department handlers ─────────────────────────────────────────────

pub async fn create_dept(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateDeptRequest>,
) -> Result<(StatusCode, Json<Department>), ApiError> {
    let dept = state.km_store.insert_dept(OrgId(org_id), body.name)?;
    Ok((StatusCode::CREATED, Json(dept)))
}

pub async fn list_depts(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Json<ListResponse<Department>> {
    let depts = state.km_store.list_depts_in_org(OrgId(org_id));
    let total = depts.len();
    Json(ListResponse { data: depts, total })
}

pub async fn get_dept(
    State(state): State<AppState>,
    Path((_org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Department>, ApiError> {
    let dept = state.km_store.get_dept(DeptId(dept_id))?;
    Ok(Json(dept))
}

pub async fn delete_dept(
    State(state): State<AppState>,
    Path((_org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let doc_ids = state.km_store.cascade_delete_dept(DeptId(dept_id))?;
    for doc_id in doc_ids {
        let _ = state.search_engine.delete_doc(doc_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Workspace handlers ──────────────────────────────────────────────

pub async fn create_workspace(
    State(state): State<AppState>,
    Path((_org_id, dept_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<Workspace>), ApiError> {
    let ws = state.km_store.insert_workspace(DeptId(dept_id), body.name)?;
    Ok((StatusCode::CREATED, Json(ws)))
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    Path((_org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Json<ListResponse<Workspace>> {
    let workspaces = state.km_store.list_workspaces_in_dept(DeptId(dept_id));
    let total = workspaces.len();
    Json(ListResponse { data: workspaces, total })
}

pub async fn get_workspace(
    State(state): State<AppState>,
    Path((_org_id, _dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Workspace>, ApiError> {
    let ws = state.km_store.get_workspace(WorkspaceId(ws_id))?;
    Ok(Json(ws))
}

pub async fn delete_workspace(
    State(state): State<AppState>,
    Path((_org_id, _dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let doc_ids = state.km_store.cascade_delete_workspace(WorkspaceId(ws_id))?;
    for doc_id in doc_ids {
        let _ = state.search_engine.delete_doc(doc_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}
