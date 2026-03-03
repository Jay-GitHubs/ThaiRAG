use axum::extract::{Path, Query, State};
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
use crate::store::scopes_match;

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

// ── Pagination ──────────────────────────────────────────────────────

fn default_limit() -> usize {
    50
}

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

pub fn paginate<T>(items: Vec<T>, params: &PaginationParams) -> (Vec<T>, usize) {
    let total = items.len();
    let data = items.into_iter().skip(params.offset).take(params.limit).collect();
    (data, total)
}

#[derive(Deserialize)]
#[serde(tag = "level")]
pub enum ScopeRequest {
    Org,
    Dept { dept_id: Uuid },
    Workspace { dept_id: Uuid, workspace_id: Uuid },
}

#[derive(Deserialize)]
pub struct GrantPermissionRequest {
    pub email: String,
    pub role: Role,
    pub scope: ScopeRequest,
}

#[derive(Deserialize)]
pub struct RevokePermissionRequest {
    pub email: String,
    pub scope: ScopeRequest,
}

#[derive(Serialize)]
pub struct PermissionResponse {
    pub user_id: String,
    pub email: String,
    pub role: Role,
    pub scope: PermissionScope,
}

#[derive(Deserialize)]
pub struct ScopedGrantRequest {
    pub email: String,
    pub role: Role,
}

#[derive(Deserialize)]
pub struct ScopedRevokeRequest {
    pub email: String,
}

// ── Permission helpers ──────────────────────────────────────────────

fn resolve_scope(org_id: OrgId, scope: ScopeRequest) -> PermissionScope {
    match scope {
        ScopeRequest::Org => PermissionScope::Org { org_id },
        ScopeRequest::Dept { dept_id } => PermissionScope::Dept {
            org_id,
            dept_id: DeptId(dept_id),
        },
        ScopeRequest::Workspace {
            dept_id,
            workspace_id,
        } => PermissionScope::Workspace {
            org_id,
            dept_id: DeptId(dept_id),
            workspace_id: WorkspaceId(workspace_id),
        },
    }
}

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
    tracing::info!(org_id = %org.id, name = %org.name, "Organization created");

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
    Query(params): Query<PaginationParams>,
) -> Json<ListResponse<Organization>> {
    let orgs = state.km_store.list_orgs();
    let (data, total) = paginate(orgs, &params);
    Json(ListResponse { data, total })
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
    tracing::info!(%org_id, "Organization deleted");
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
    tracing::info!(dept_id = %dept.id, %org_id, name = %dept.name, "Department created");
    Ok((StatusCode::CREATED, Json(dept)))
}

pub async fn list_depts(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<Department>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "list departments")?;
    let depts = state.km_store.list_depts_in_org(org_id);
    let (data, total) = paginate(depts, &params);
    Ok(Json(ListResponse { data, total }))
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
    tracing::info!(%dept_id, "Department deleted");
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
    tracing::info!(ws_id = %ws.id, %dept_id, name = %ws.name, "Workspace created");
    Ok((StatusCode::CREATED, Json(ws)))
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<Workspace>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "list workspaces")?;
    let workspaces = state.km_store.list_workspaces_in_dept(DeptId(dept_id));
    let (data, total) = paginate(workspaces, &params);
    Ok(Json(ListResponse { data, total }))
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
    tracing::info!(%ws_id, "Workspace deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ── Permission management handlers ─────────────────────────────────

pub async fn grant_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<GrantPermissionRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_manage, "manage permissions")?;

    // Role ceiling: non-Owners can only grant roles strictly below their own
    if let PermCheck::Role(caller_role) = &perm {
        if *caller_role != Role::Owner && body.role >= *caller_role {
            return Err(ApiError(ThaiRagError::Authorization(
                "Cannot grant a role equal to or above your own".into(),
            )));
        }
    }

    let target = state.km_store.get_user_by_email(&body.email)?;
    let scope = resolve_scope(org_id, body.scope);

    state.km_store.upsert_permission(UserPermission {
        user_id: target.user.id,
        scope,
        role: body.role,
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<PermissionResponse>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_manage, "manage permissions")?;

    let all_perms = state.km_store.list_permissions_for_org(org_id);
    list_permissions_inner(&state, all_perms, |_| true, &params)
}

fn list_permissions_inner(
    state: &AppState,
    perms: Vec<UserPermission>,
    scope_filter: impl Fn(&PermissionScope) -> bool,
    params: &PaginationParams,
) -> Result<Json<ListResponse<PermissionResponse>>, ApiError> {
    let data: Vec<PermissionResponse> = perms
        .into_iter()
        .filter(|p| scope_filter(&p.scope))
        .map(|p| {
            let email = state
                .km_store
                .get_user(p.user_id)
                .map(|u| u.email)
                .unwrap_or_default();
            PermissionResponse {
                user_id: p.user_id.0.to_string(),
                email,
                role: p.role,
                scope: p.scope,
            }
        })
        .collect();
    let (data, total) = paginate(data, params);
    Ok(Json(ListResponse { data, total }))
}

pub async fn revoke_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<RevokePermissionRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_manage, "manage permissions")?;

    let target = state.km_store.get_user_by_email(&body.email)?;
    let scope = resolve_scope(org_id, body.scope);

    // Last-owner safety: if revoking an Owner at Org scope, ensure at least one remains
    if matches!(&scope, PermissionScope::Org { .. }) {
        let is_owner = state
            .km_store
            .list_permissions_for_org(org_id)
            .iter()
            .any(|p| {
                p.user_id == target.user.id
                    && p.role == Role::Owner
                    && scopes_match(&p.scope, &scope)
            });
        if is_owner && state.km_store.count_org_owners(org_id) <= 1 {
            return Err(ApiError(ThaiRagError::Validation(
                "Cannot revoke the last org-level Owner".into(),
            )));
        }
    }

    state.km_store.remove_permission(target.user.id, &scope)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Inner helpers for scoped permission handlers ────────────────────

fn grant_permission_inner(
    state: &AppState,
    perm: &PermCheck,
    scope: PermissionScope,
    email: &str,
    role: Role,
) -> Result<StatusCode, ApiError> {
    require(perm, Role::can_manage, "manage permissions")?;

    // Role ceiling: non-Owners can only grant roles strictly below their own
    if let PermCheck::Role(caller_role) = perm {
        if *caller_role != Role::Owner && role >= *caller_role {
            return Err(ApiError(ThaiRagError::Authorization(
                "Cannot grant a role equal to or above your own".into(),
            )));
        }
    }

    let target = state.km_store.get_user_by_email(email)?;
    state.km_store.upsert_permission(UserPermission {
        user_id: target.user.id,
        scope,
        role,
    });

    Ok(StatusCode::NO_CONTENT)
}

fn revoke_permission_inner(
    state: &AppState,
    perm: &PermCheck,
    org_id: OrgId,
    scope: PermissionScope,
    email: &str,
) -> Result<StatusCode, ApiError> {
    require(perm, Role::can_manage, "manage permissions")?;

    let target = state.km_store.get_user_by_email(email)?;

    // Last-owner safety: if revoking an Owner at Org scope, ensure at least one remains
    if matches!(&scope, PermissionScope::Org { .. }) {
        let is_owner = state
            .km_store
            .list_permissions_for_org(org_id)
            .iter()
            .any(|p| {
                p.user_id == target.user.id
                    && p.role == Role::Owner
                    && scopes_match(&p.scope, &scope)
            });
        if is_owner && state.km_store.count_org_owners(org_id) <= 1 {
            return Err(ApiError(ThaiRagError::Validation(
                "Cannot revoke the last org-level Owner".into(),
            )));
        }
    }

    state.km_store.remove_permission(target.user.id, &scope)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Department-scoped permission handlers ───────────────────────────

pub async fn grant_dept_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ScopedGrantRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    let scope = PermissionScope::Dept {
        org_id,
        dept_id: DeptId(dept_id),
    };
    grant_permission_inner(&state, &perm, scope, &body.email, body.role)
}

pub async fn list_dept_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<PermissionResponse>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_manage, "manage permissions")?;
    let dept_id = DeptId(dept_id);
    let all_perms = state.km_store.list_permissions_for_org(org_id);
    list_permissions_inner(&state, all_perms, |s| {
        matches!(s, PermissionScope::Dept { dept_id: did, .. } if *did == dept_id)
    }, &params)
}

pub async fn revoke_dept_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ScopedRevokeRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    let scope = PermissionScope::Dept {
        org_id,
        dept_id: DeptId(dept_id),
    };
    revoke_permission_inner(&state, &perm, org_id, scope, &body.email)
}

// ── Workspace-scoped permission handlers ────────────────────────────

pub async fn grant_workspace_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<ScopedGrantRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    let scope = PermissionScope::Workspace {
        org_id,
        dept_id: DeptId(dept_id),
        workspace_id: WorkspaceId(ws_id),
    };
    grant_permission_inner(&state, &perm, scope, &body.email, body.role)
}

pub async fn list_workspace_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, _dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<PermissionResponse>>, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_manage, "manage permissions")?;
    let ws_id = WorkspaceId(ws_id);
    let all_perms = state.km_store.list_permissions_for_org(org_id);
    list_permissions_inner(&state, all_perms, |s| {
        matches!(s, PermissionScope::Workspace { workspace_id, .. } if *workspace_id == ws_id)
    }, &params)
}

pub async fn revoke_workspace_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<ScopedRevokeRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    let scope = PermissionScope::Workspace {
        org_id,
        dept_id: DeptId(dept_id),
        workspace_id: WorkspaceId(ws_id),
    };
    revoke_permission_inner(&state, &perm, org_id, scope, &body.email)
}
