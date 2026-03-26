use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, Organization, PermissionScope, User, UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{DeptId, OrgId, UserId, WorkspaceId};

use crate::app_state::AppState;
use crate::audit::{AuditAction, audit_log};
use crate::error::{ApiError, AppJson};
use crate::store::{scope_org_id, scopes_match};

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
    let data = items
        .into_iter()
        .skip(params.offset)
        .take(params.limit)
        .collect();
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

pub type PermCheckPublic = PermCheck;

pub enum PermCheck {
    AuthDisabled,
    SuperAdmin,
    Role(Role),
    NoPermission,
}

fn user_id_from_claims(claims: &AuthClaims) -> Option<UserId> {
    claims.sub.parse::<Uuid>().ok().map(UserId)
}

fn is_super_admin(state: &AppState, user_id: UserId) -> bool {
    state
        .km_store
        .get_user(user_id)
        .map(|u| u.is_super_admin || u.role == "super_admin")
        .unwrap_or(false)
}

/// Check permission at org level (considers all scopes within the org).
fn resolve_perm(claims: &AuthClaims, state: &AppState, org_id: OrgId) -> PermCheck {
    if claims.sub == "anonymous" {
        return PermCheck::AuthDisabled;
    }
    let Some(user_id) = user_id_from_claims(claims) else {
        return PermCheck::NoPermission;
    };
    if is_super_admin(state, user_id) {
        return PermCheck::SuperAdmin;
    }
    match state.km_store.get_user_role_for_org(user_id, org_id) {
        Some(role) => PermCheck::Role(role),
        None => PermCheck::NoPermission,
    }
}

/// Check permission at dept level (org + matching dept scope).
fn resolve_perm_dept(
    claims: &AuthClaims,
    state: &AppState,
    org_id: OrgId,
    dept_id: DeptId,
) -> PermCheck {
    if claims.sub == "anonymous" {
        return PermCheck::AuthDisabled;
    }
    let Some(user_id) = user_id_from_claims(claims) else {
        return PermCheck::NoPermission;
    };
    if is_super_admin(state, user_id) {
        return PermCheck::SuperAdmin;
    }
    match state
        .km_store
        .get_user_role_for_dept(user_id, org_id, dept_id)
    {
        Some(role) => PermCheck::Role(role),
        None => PermCheck::NoPermission,
    }
}

/// Check permission at workspace level (org + dept + matching ws scope).
pub fn resolve_perm_ws(
    claims: &AuthClaims,
    state: &AppState,
    org_id: OrgId,
    dept_id: DeptId,
    workspace_id: WorkspaceId,
) -> PermCheck {
    if claims.sub == "anonymous" {
        return PermCheck::AuthDisabled;
    }
    let Some(user_id) = user_id_from_claims(claims) else {
        return PermCheck::NoPermission;
    };
    if is_super_admin(state, user_id) {
        return PermCheck::SuperAdmin;
    }
    match state
        .km_store
        .get_user_role_for_workspace(user_id, org_id, dept_id, workspace_id)
    {
        Some(role) => PermCheck::Role(role),
        None => PermCheck::NoPermission,
    }
}

fn require(perm: &PermCheck, check: fn(&Role) -> bool, action: &str) -> Result<(), ApiError> {
    match perm {
        PermCheck::AuthDisabled | PermCheck::SuperAdmin => Ok(()),
        PermCheck::Role(role) if check(role) => Ok(()),
        PermCheck::Role(_) | PermCheck::NoPermission => Err(ApiError(ThaiRagError::Authorization(
            format!("Insufficient permission: {action}"),
        ))),
    }
}

fn is_bypassed(perm: &PermCheck) -> bool {
    matches!(perm, PermCheck::AuthDisabled | PermCheck::SuperAdmin)
}

// ── Organization handlers ───────────────────────────────────────────

pub async fn create_org(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<CreateOrgRequest>,
) -> Result<(StatusCode, Json<Organization>), ApiError> {
    let org = state.km_store.insert_org(body.name)?;
    tracing::info!(org_id = %org.id, name = %org.name, "Organization created");

    // Auto-grant Owner to creator
    if claims.sub != "anonymous"
        && let Some(user_id) = user_id_from_claims(&claims)
    {
        state.km_store.add_permission(UserPermission {
            user_id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Owner,
        });
    }

    Ok((StatusCode::CREATED, Json(org)))
}

pub async fn list_orgs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<PaginationParams>,
) -> Json<ListResponse<Organization>> {
    let all_orgs = state.km_store.list_orgs();

    let orgs = if claims.sub == "anonymous" {
        all_orgs
    } else if let Some(user_id) = user_id_from_claims(&claims) {
        if is_super_admin(&state, user_id) {
            all_orgs
        } else {
            let perms = state.km_store.list_user_permissions(user_id);
            let accessible_org_ids: HashSet<OrgId> =
                perms.iter().map(|p| scope_org_id(&p.scope)).collect();
            all_orgs
                .into_iter()
                .filter(|o| accessible_org_ids.contains(&o.id))
                .collect()
        }
    } else {
        vec![]
    };

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
        let _ = state.providers().search_engine.delete_doc(doc_id).await;
    }
    tracing::info!(%org_id, "Organization deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ── Department handlers ─────────────────────────────────────────────

pub async fn create_dept(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    AppJson(body): AppJson<CreateDeptRequest>,
) -> Result<(StatusCode, Json<Department>), ApiError> {
    let org_id = OrgId(org_id);
    // Creating a dept is an org-level operation — need org-level perm
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
    // Must have some permission in this org to list depts
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "list departments")?;

    let all_depts = state.km_store.list_depts_in_org(org_id);

    let depts = if is_bypassed(&perm) {
        all_depts
    } else if let Some(user_id) = user_id_from_claims(&claims) {
        let perms = state.km_store.list_user_permissions(user_id);
        // Org-level perm → see all depts
        let has_org_perm = perms
            .iter()
            .any(|p| matches!(&p.scope, PermissionScope::Org { org_id: oid } if *oid == org_id));
        if has_org_perm {
            all_depts
        } else {
            // Collect dept_ids from dept-level and workspace-level perms
            let mut dept_ids: HashSet<DeptId> = HashSet::new();
            for p in &perms {
                match &p.scope {
                    PermissionScope::Dept {
                        org_id: oid,
                        dept_id,
                    } if *oid == org_id => {
                        dept_ids.insert(*dept_id);
                    }
                    PermissionScope::Workspace {
                        org_id: oid,
                        dept_id,
                        ..
                    } if *oid == org_id => {
                        dept_ids.insert(*dept_id);
                    }
                    _ => {}
                }
            }
            all_depts
                .into_iter()
                .filter(|d| dept_ids.contains(&d.id))
                .collect()
        }
    } else {
        vec![]
    };

    let (data, total) = paginate(depts, &params);
    Ok(Json(ListResponse { data, total }))
}

pub async fn get_dept(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Department>, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id);
    require(&perm, Role::can_read, "read department")?;
    let dept = state.km_store.get_dept(dept_id)?;
    Ok(Json(dept))
}

pub async fn delete_dept(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id);
    require(&perm, Role::can_delete, "delete department")?;
    let doc_ids = state.km_store.cascade_delete_dept(dept_id)?;
    for doc_id in doc_ids {
        let _ = state.providers().search_engine.delete_doc(doc_id).await;
    }
    tracing::info!(%dept_id, "Department deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ── Workspace handlers ──────────────────────────────────────────────

pub async fn create_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    AppJson(body): AppJson<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<Workspace>), ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    // Creating a workspace is a dept-level operation
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id);
    require(&perm, Role::can_write, "create workspace")?;
    let ws = state.km_store.insert_workspace(dept_id, body.name)?;
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
    let dept_id = DeptId(dept_id);
    // Must have some access to this dept (or its children)
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_read, "list workspaces")?;

    let all_ws = state.km_store.list_workspaces_in_dept(dept_id);

    let workspaces = if is_bypassed(&perm) {
        all_ws
    } else if let Some(user_id) = user_id_from_claims(&claims) {
        let perms = state.km_store.list_user_permissions(user_id);
        let has_org_perm = perms
            .iter()
            .any(|p| matches!(&p.scope, PermissionScope::Org { org_id: oid } if *oid == org_id));
        let has_dept_perm = perms.iter().any(|p| {
            matches!(&p.scope, PermissionScope::Dept { org_id: oid, dept_id: did } if *oid == org_id && *did == dept_id)
        });
        if has_org_perm || has_dept_perm {
            all_ws
        } else {
            let ws_ids: HashSet<WorkspaceId> = perms
                .iter()
                .filter_map(|p| match &p.scope {
                    PermissionScope::Workspace {
                        org_id: oid,
                        dept_id: did,
                        workspace_id,
                    } if *oid == org_id && *did == dept_id => Some(*workspace_id),
                    _ => None,
                })
                .collect();
            all_ws
                .into_iter()
                .filter(|w| ws_ids.contains(&w.id))
                .collect()
        }
    } else {
        vec![]
    };

    let (data, total) = paginate(workspaces, &params);
    Ok(Json(ListResponse { data, total }))
}

pub async fn get_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Workspace>, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let workspace_id = WorkspaceId(ws_id);
    let perm = resolve_perm_ws(&claims, &state, org_id, dept_id, workspace_id);
    require(&perm, Role::can_read, "read workspace")?;
    let ws = state.km_store.get_workspace(workspace_id)?;
    Ok(Json(ws))
}

pub async fn delete_workspace(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    // Deleting a workspace is a dept-level operation
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id);
    require(&perm, Role::can_delete, "delete workspace")?;
    let doc_ids = state
        .km_store
        .cascade_delete_workspace(WorkspaceId(ws_id))?;
    for doc_id in doc_ids {
        let _ = state.providers().search_engine.delete_doc(doc_id).await;
    }
    tracing::info!(%ws_id, "Workspace deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ── Permission management handlers ─────────────────────────────────

pub async fn grant_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(org_id): Path<Uuid>,
    AppJson(body): AppJson<GrantPermissionRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let perm = resolve_perm(&claims, &state, org_id);
    require(&perm, Role::can_manage, "manage permissions")?;

    // Role ceiling: non-Owners can only grant roles strictly below their own
    if let PermCheck::Role(caller_role) = &perm
        && *caller_role != Role::Owner
        && body.role >= *caller_role
    {
        return Err(ApiError(ThaiRagError::Authorization(
            "Cannot grant a role equal to or above your own".into(),
        )));
    }

    let target = state.km_store.get_user_by_email(&body.email)?;
    let scope = resolve_scope(org_id, body.scope);

    state.km_store.upsert_permission(UserPermission {
        user_id: target.user.id,
        scope: scope.clone(),
        role: body.role,
    });
    audit_log(
        &state.km_store,
        &claims.sub,
        AuditAction::PermissionGranted,
        &body.email,
        true,
        Some(&format!("role={:?} scope={scope:?}", body.role)),
    );

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
    AppJson(body): AppJson<RevokePermissionRequest>,
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
    audit_log(
        &state.km_store,
        &claims.sub,
        AuditAction::PermissionRevoked,
        &body.email,
        true,
        Some(&format!("scope={scope:?}")),
    );
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
    if let PermCheck::Role(caller_role) = perm
        && *caller_role != Role::Owner
        && role >= *caller_role
    {
        return Err(ApiError(ThaiRagError::Authorization(
            "Cannot grant a role equal to or above your own".into(),
        )));
    }

    let target = state.km_store.get_user_by_email(email)?;
    state.km_store.upsert_permission(UserPermission {
        user_id: target.user.id,
        scope: scope.clone(),
        role,
    });
    audit_log(
        &state.km_store,
        "admin",
        AuditAction::PermissionGranted,
        email,
        true,
        Some(&format!("role={role:?} scope={scope:?}")),
    );

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

    // Security: Clear the user's sessions and conversation memory to prevent
    // stale context from leaking revoked document content (session history,
    // context compaction summaries, and conversation memories may contain
    // information from workspaces the user no longer has access to).
    let cleared = state.session_store.clear_user_sessions(target.user.id);
    let memory_key = format!("memory:{}", target.user.id.0);
    state.km_store.delete_setting(&memory_key);
    if cleared > 0 {
        tracing::info!(
            user_id = %target.user.id,
            sessions_cleared = cleared,
            "Cleared user sessions after permission revocation"
        );
    }

    audit_log(
        &state.km_store,
        "admin",
        AuditAction::PermissionRevoked,
        email,
        true,
        Some(&format!("scope={scope:?}")),
    );
    Ok(StatusCode::NO_CONTENT)
}

// ── Department-scoped permission handlers ───────────────────────────

pub async fn grant_dept_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    AppJson(body): AppJson<ScopedGrantRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id);
    let scope = PermissionScope::Dept { org_id, dept_id };
    grant_permission_inner(&state, &perm, scope, &body.email, body.role)
}

pub async fn list_dept_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<PermissionResponse>>, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id_typed = DeptId(dept_id);
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id_typed);
    require(&perm, Role::can_manage, "manage permissions")?;
    let all_perms = state.km_store.list_permissions_for_org(org_id);
    list_permissions_inner(
        &state,
        all_perms,
        |s| matches!(s, PermissionScope::Dept { dept_id: did, .. } if *did == dept_id_typed),
        &params,
    )
}

pub async fn revoke_dept_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id)): Path<(Uuid, Uuid)>,
    AppJson(body): AppJson<ScopedRevokeRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let perm = resolve_perm_dept(&claims, &state, org_id, dept_id);
    let scope = PermissionScope::Dept { org_id, dept_id };
    revoke_permission_inner(&state, &perm, org_id, scope, &body.email)
}

// ── Workspace-scoped permission handlers ────────────────────────────

pub async fn grant_workspace_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
    AppJson(body): AppJson<ScopedGrantRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let workspace_id = WorkspaceId(ws_id);
    let perm = resolve_perm_ws(&claims, &state, org_id, dept_id, workspace_id);
    let scope = PermissionScope::Workspace {
        org_id,
        dept_id,
        workspace_id,
    };
    grant_permission_inner(&state, &perm, scope, &body.email, body.role)
}

pub async fn list_workspace_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<PermissionResponse>>, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let ws_id_typed = WorkspaceId(ws_id);
    let perm = resolve_perm_ws(&claims, &state, org_id, dept_id, ws_id_typed);
    require(&perm, Role::can_manage, "manage permissions")?;
    let all_perms = state.km_store.list_permissions_for_org(org_id);
    list_permissions_inner(
        &state,
        all_perms,
        |s| matches!(s, PermissionScope::Workspace { workspace_id, .. } if *workspace_id == ws_id_typed),
        &params,
    )
}

pub async fn revoke_workspace_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((org_id, dept_id, ws_id)): Path<(Uuid, Uuid, Uuid)>,
    AppJson(body): AppJson<ScopedRevokeRequest>,
) -> Result<StatusCode, ApiError> {
    let org_id = OrgId(org_id);
    let dept_id = DeptId(dept_id);
    let perm = resolve_perm_ws(&claims, &state, org_id, dept_id, WorkspaceId(ws_id));
    let scope = PermissionScope::Workspace {
        org_id,
        dept_id,
        workspace_id: WorkspaceId(ws_id),
    };
    revoke_permission_inner(&state, &perm, org_id, scope, &body.email)
}

// ── Users handler ───────────────────────────────────────────────────

pub async fn list_users(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(params): Query<PaginationParams>,
) -> Json<ListResponse<User>> {
    let users = state.km_store.list_users();
    let (data, total) = paginate(users, &params);
    Json(ListResponse { data, total })
}

// ── Update user role ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

pub async fn update_user_role(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(user_id): Path<Uuid>,
    AppJson(body): AppJson<UpdateUserRoleRequest>,
) -> Result<Json<User>, ApiError> {
    // Only super_admin can change roles
    let caller_id: Uuid = claims
        .sub
        .parse()
        .map_err(|_| ApiError(ThaiRagError::Validation("Invalid user ID".into())))?;
    let caller = state.km_store.get_user(UserId(caller_id))?;
    if !caller.is_super_admin && caller.role != "super_admin" {
        return Err(ApiError(ThaiRagError::Authorization(
            "Only super admins can update user roles".into(),
        )));
    }

    let valid_roles = ["viewer", "editor", "admin", "super_admin"];
    if !valid_roles.contains(&body.role.as_str()) {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid role '{}'. Must be one of: {}",
            body.role,
            valid_roles.join(", ")
        ))));
    }

    let is_super = body.role == "super_admin";
    let target = state.km_store.get_user(UserId(user_id))?;
    let record = state.km_store.get_user_by_email(&target.email)?;

    state.km_store.upsert_user_by_email(
        target.email.clone(),
        target.name.clone(),
        record.password_hash.clone(),
        is_super,
        body.role.clone(),
    )?;

    let updated = state.km_store.get_user(UserId(user_id))?;
    audit_log(
        &state.km_store,
        &claims.sub,
        AuditAction::SettingsChanged,
        &format!("User {} role changed to {}", target.email, body.role),
        true,
        None,
    );
    tracing::info!(%user_id, role = %body.role, "User role updated");
    Ok(Json(updated))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let user = state.km_store.get_user(UserId(user_id))?;
    if user.is_super_admin {
        return Err(ApiError(ThaiRagError::Validation(
            "Cannot delete a super admin user".into(),
        )));
    }
    state.km_store.delete_user(UserId(user_id))?;
    audit_log(
        &state.km_store,
        &_claims.sub,
        AuditAction::UserDeleted,
        &user.email,
        true,
        None,
    );
    tracing::info!(%user_id, "User deleted");
    Ok(StatusCode::NO_CONTENT)
}
