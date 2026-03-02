use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::models::{Department, Organization, PermissionScope, UserPermission, Workspace};
use thairag_core::permission::Role;
use thairag_core::types::{DeptId, OrgId, UserId, WorkspaceId};
use thairag_core::ThaiRagError;

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

// ── Permission helpers ──────────────────────────────────────────────

enum PermCheck {
    AuthDisabled,
    Role(Role),
    NoPermission,
}

fn resolve_perm(claims: &AuthClaims, state: &AppState, org_id: OrgId) -> PermCheck {
    if claims.sub == "anonymous" {
        return PermCheck::AuthDisabled;
    }
    let Ok(user_id) = claims.sub.parse::<Uuid>() else {
        return PermCheck::NoPermission;
    };
    match state
        .km_store
        .get_user_role_for_org(UserId(user_id), org_id)
    {
        Some(role) => PermCheck::Role(role),
        None => PermCheck::NoPermission,
    }
}

fn require(perm: &PermCheck, check: fn(&Role) -> bool, action: &str) -> Result<(), ApiError> {
    match perm {
        PermCheck::AuthDisabled => Ok(()),
        PermCheck::Role(role) if check(role) => Ok(()),
        PermCheck::Role(_) | PermCheck::NoPermission => Err(ApiError(
            ThaiRagError::Authorization(format!("Insufficient permission: {action}")),
        )),
    }
}

// ── Organization handlers ───────────────────────────────────────────

pub async fn create_org(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(body): Json<CreateOrgRequest>,
) -> Result<(StatusCode, Json<Organization>), ApiError> {
    let org = state.km_store.insert_org(body.name)?;

    // Auto-grant Owner to creator
    if claims.sub != "anonymous" {
        if let Ok(user_id) = claims.sub.parse::<Uuid>() {
            state.km_store.add_permission(UserPermission {
                user_id: UserId(user_id),
                scope: PermissionScope::Org { org_id: org.id },
                role: Role::Owner,
            });
        }
    }

    Ok((StatusCode::CREATED, Json(org)))
}

pub async fn list_orgs(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
) -> Json<ListResponse<Organization>> {
    let orgs = state.km_store.list_orgs();
    let total = orgs.len();
    Json(ListResponse { data: orgs, total })
}

pub async fn get_org(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Organization>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "read org")?;
    let org = state.km_store.get_org(org_id)?;
    Ok(Json(org))
}

pub async fn delete_org(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_delete, "delete org")?;
    let doc_ids = state.km_store.cascade_delete_org(org_id)?;
    for doc_id in doc_ids {
        let _ = state.search_engine.delete_doc(doc_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Department handlers ─────────────────────────────────────────────

pub async fn create_dept(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateDeptRequest>,
) -> Result<(StatusCode, Json<Department>), ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_write, "create department")?;
    let dept = state.km_store.insert_dept(org_id, body.name)?;
    Ok((StatusCode::CREATED, Json(dept)))
}

pub async fn list_depts(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<ListResponse<Department>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "list departments")?;
    let depts = state.km_store.list_depts_in_org(org_id);
    let total = depts.len();
    Ok(Json(ListResponse { data: depts, total }))
}

pub async fn get_dept(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Department>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "read department")?;
    let dept = state.km_store.get_dept(DeptId(dept_id))?;
    Ok(Json(dept))
}

pub async fn delete_dept(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_delete, "delete department")?;
    let doc_ids = state.km_store.cascade_delete_dept(DeptId(dept_id))?;
    for doc_id in doc_ids {
        let _ = state.search_engine.delete_doc(doc_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Workspace handlers ──────────────────────────────────────────────

pub async fn create_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<Workspace>), ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_write, "create workspace")?;
    let ws = state.km_store.insert_workspace(DeptId(dept_id), body.name)?;
    Ok((StatusCode::CREATED, Json(ws)))
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ListResponse<Workspace>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "list workspaces")?;
    let workspaces = state.km_store.list_workspaces_in_dept(DeptId(dept_id));
    let total = workspaces.len();
    Ok(Json(ListResponse {
        data: workspaces,
        total,
    }))
}

pub async fn get_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, _dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Workspace>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "read workspace")?;
    let ws = state.km_store.get_workspace(WorkspaceId(ws_id))?;
    Ok(Json(ws))
}

pub async fn delete_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, _dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_delete, "delete workspace")?;
    let doc_ids = state
        .km_store
        .cascade_delete_workspace(WorkspaceId(ws_id))?;
    for doc_id in doc_ids {
        let _ = state.search_engine.delete_doc(doc_id).await;
    }
    Ok(StatusCode::NO_CONTENT)
}
