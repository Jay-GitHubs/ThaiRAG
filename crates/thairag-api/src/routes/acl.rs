use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::{AclPermission, DocId, UserId, WorkspaceId};
use tracing::info;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};

use super::km::{is_super_admin_pub, user_id_from_claims_pub};

// ── Request / Response Types ────────────────────────────────────────

#[derive(Deserialize)]
pub struct GrantWorkspaceAclRequest {
    pub user_id: Uuid,
    pub permission: String,
}

#[derive(Deserialize)]
pub struct GrantDocumentAclRequest {
    pub user_id: Uuid,
    pub permission: String,
}

#[derive(Serialize)]
pub struct AclEntry {
    pub user_id: Uuid,
    pub permission: String,
    pub granted_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_by: Option<Uuid>,
}

#[derive(Serialize)]
pub struct AclListResponse {
    pub acls: Vec<AclEntry>,
}

// ── Access Checker ──────────────────────────────────────────────────

/// Check whether a user has at least the required ACL permission on a workspace.
/// Super admins bypass all checks. Org admins have implicit Admin access.
pub fn check_workspace_access(
    state: &AppState,
    claims: &AuthClaims,
    workspace_id: WorkspaceId,
    required: AclPermission,
) -> Result<(), ApiError> {
    if claims.sub == "anonymous" {
        // Auth disabled — allow all
        return Ok(());
    }
    let Some(user_id) = user_id_from_claims_pub(claims) else {
        return Err(ApiError(ThaiRagError::Authorization(
            "Invalid user identity".into(),
        )));
    };
    // Super admin bypasses all checks
    if is_super_admin_pub(state, user_id) {
        return Ok(());
    }
    // Check org admin — implicit Admin on all workspaces in their org
    if let Ok(org_id) = state.km_store.org_id_for_workspace(workspace_id)
        && let Some(role) = state.km_store.get_user_role_for_org(user_id, org_id)
        && role.can_manage()
    {
        return Ok(());
    }
    // Check workspace ACL
    if let Some(perm) = state.km_store.get_user_workspace_acl(user_id, workspace_id)
        && perm >= required
    {
        return Ok(());
    }
    Err(ApiError(ThaiRagError::Authorization(format!(
        "Insufficient workspace permission: requires {required}"
    ))))
}

/// Require workspace Admin permission (for managing ACLs).
fn require_acl_admin(
    state: &AppState,
    claims: &AuthClaims,
    workspace_id: WorkspaceId,
) -> Result<(), ApiError> {
    if claims.sub == "anonymous" {
        return Ok(());
    }
    let Some(user_id) = user_id_from_claims_pub(claims) else {
        return Err(ApiError(ThaiRagError::Authorization(
            "Invalid user identity".into(),
        )));
    };
    if is_super_admin_pub(state, user_id) {
        return Ok(());
    }
    // Org admin has implicit workspace Admin
    if let Ok(org_id) = state.km_store.org_id_for_workspace(workspace_id)
        && let Some(role) = state.km_store.get_user_role_for_org(user_id, org_id)
        && role.can_manage()
    {
        return Ok(());
    }
    // Workspace ACL admin
    if let Some(perm) = state.km_store.get_user_workspace_acl(user_id, workspace_id)
        && perm >= AclPermission::Admin
    {
        return Ok(());
    }
    Err(ApiError(ThaiRagError::Authorization(
        "Only workspace admins or super admins can manage ACLs".into(),
    )))
}

// ── Workspace ACL Endpoints ─────────────────────────────────────────

pub async fn grant_workspace_acl(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(ws_id): Path<Uuid>,
    AppJson(body): AppJson<GrantWorkspaceAclRequest>,
) -> Result<(StatusCode, Json<AclEntry>), ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    require_acl_admin(&state, &claims, workspace_id)?;

    let permission = parse_acl_permission(&body.permission)?;
    let target_user_id = UserId(body.user_id);
    let granter = user_id_from_claims_pub(&claims);

    // Verify target user exists
    state
        .km_store
        .get_user(target_user_id)
        .map_err(|_| ApiError(ThaiRagError::NotFound("Target user not found".into())))?;

    let acl =
        state
            .km_store
            .grant_workspace_access(target_user_id, workspace_id, permission, granter)?;

    info!(
        user_id = %target_user_id,
        workspace_id = %workspace_id,
        permission = %permission,
        "Granted workspace ACL"
    );

    Ok((
        StatusCode::CREATED,
        Json(AclEntry {
            user_id: acl.user_id.0,
            permission: acl.permission.as_str().to_string(),
            granted_at: acl.granted_at,
            granted_by: acl.granted_by.map(|u| u.0),
        }),
    ))
}

pub async fn list_workspace_acls(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(ws_id): Path<Uuid>,
) -> Result<Json<AclListResponse>, ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    // Any user with at least Read access can view ACLs
    check_workspace_access(&state, &claims, workspace_id, AclPermission::Read)?;

    let acls = state.km_store.list_workspace_acls(workspace_id);
    Ok(Json(AclListResponse {
        acls: acls
            .into_iter()
            .map(|a| AclEntry {
                user_id: a.user_id.0,
                permission: a.permission.as_str().to_string(),
                granted_at: a.granted_at,
                granted_by: a.granted_by.map(|u| u.0),
            })
            .collect(),
    }))
}

pub async fn revoke_workspace_acl(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((ws_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    require_acl_admin(&state, &claims, workspace_id)?;

    let user_id = UserId(target_user_id);
    state
        .km_store
        .revoke_workspace_access(user_id, workspace_id)?;

    info!(
        user_id = %user_id,
        workspace_id = %workspace_id,
        "Revoked workspace ACL"
    );

    Ok(StatusCode::NO_CONTENT)
}

// ── Document ACL Endpoints ──────────────────────────────────────────

pub async fn grant_document_acl(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    AppJson(body): AppJson<GrantDocumentAclRequest>,
) -> Result<(StatusCode, Json<AclEntry>), ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    // Only workspace admins can grant document-level ACLs
    require_acl_admin(&state, &claims, workspace_id)?;

    let permission = parse_doc_acl_permission(&body.permission)?;
    let target_user_id = UserId(body.user_id);
    let doc = DocId(doc_id);

    // Verify the document belongs to the workspace
    let document = state.km_store.get_document(doc).map_err(ApiError)?;
    if document.workspace_id != workspace_id {
        return Err(ApiError(ThaiRagError::NotFound(
            "Document not found in this workspace".into(),
        )));
    }

    // Verify target user exists
    state
        .km_store
        .get_user(target_user_id)
        .map_err(|_| ApiError(ThaiRagError::NotFound("Target user not found".into())))?;

    let acl = state
        .km_store
        .grant_document_access(target_user_id, doc, permission)?;

    info!(
        user_id = %target_user_id,
        doc_id = %doc_id,
        permission = %permission,
        "Granted document ACL"
    );

    Ok((
        StatusCode::CREATED,
        Json(AclEntry {
            user_id: acl.user_id.0,
            permission: acl.permission.as_str().to_string(),
            granted_at: acl.granted_at,
            granted_by: None,
        }),
    ))
}

pub async fn revoke_document_acl(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((ws_id, doc_id, target_user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let workspace_id = WorkspaceId(ws_id);
    require_acl_admin(&state, &claims, workspace_id)?;

    let doc = DocId(doc_id);
    let user_id = UserId(target_user_id);

    // Verify the document belongs to the workspace
    let document = state.km_store.get_document(doc).map_err(ApiError)?;
    if document.workspace_id != workspace_id {
        return Err(ApiError(ThaiRagError::NotFound(
            "Document not found in this workspace".into(),
        )));
    }

    state.km_store.revoke_document_access(user_id, doc)?;

    info!(
        user_id = %user_id,
        doc_id = %doc_id,
        "Revoked document ACL"
    );

    Ok(StatusCode::NO_CONTENT)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn parse_acl_permission(s: &str) -> Result<AclPermission, ApiError> {
    match s {
        "read" => Ok(AclPermission::Read),
        "write" => Ok(AclPermission::Write),
        "admin" => Ok(AclPermission::Admin),
        _ => Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid permission: '{s}'. Must be one of: read, write, admin"
        )))),
    }
}

fn parse_doc_acl_permission(s: &str) -> Result<AclPermission, ApiError> {
    match s {
        "read" => Ok(AclPermission::Read),
        "write" => Ok(AclPermission::Write),
        _ => Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid document permission: '{s}'. Must be one of: read, write"
        )))),
    }
}
