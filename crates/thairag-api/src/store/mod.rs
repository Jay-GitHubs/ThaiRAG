pub mod memory;
pub mod sqlite;

use thairag_core::models::{
    Department, Document, Organization, PermissionScope, User, UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{DeptId, DocId, OrgId, UserId, WorkspaceId};
use thairag_core::ThaiRagError;

type Result<T> = std::result::Result<T, ThaiRagError>;

/// Check whether two `PermissionScope` values target the same entity.
pub fn scopes_match(a: &PermissionScope, b: &PermissionScope) -> bool {
    match (a, b) {
        (PermissionScope::Org { org_id: a }, PermissionScope::Org { org_id: b }) => a == b,
        (
            PermissionScope::Dept {
                org_id: ao,
                dept_id: ad,
            },
            PermissionScope::Dept {
                org_id: bo,
                dept_id: bd,
            },
        ) => ao == bo && ad == bd,
        (
            PermissionScope::Workspace {
                org_id: ao,
                dept_id: ad,
                workspace_id: aw,
            },
            PermissionScope::Workspace {
                org_id: bo,
                dept_id: bd,
                workspace_id: bw,
            },
        ) => ao == bo && ad == bd && aw == bw,
        _ => false,
    }
}

pub fn scope_org_id(scope: &PermissionScope) -> OrgId {
    match scope {
        PermissionScope::Org { org_id } => *org_id,
        PermissionScope::Dept { org_id, .. } => *org_id,
        PermissionScope::Workspace { org_id, .. } => *org_id,
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub user: User,
    pub password_hash: String,
}

/// Trait abstracting the KM store. All methods are synchronous (`Send + Sync`).
pub trait KmStoreTrait: Send + Sync {
    // ── Organization ────────────────────────────────────────────────
    fn insert_org(&self, name: String) -> Result<Organization>;
    fn get_org(&self, id: OrgId) -> Result<Organization>;
    fn list_orgs(&self) -> Vec<Organization>;
    fn delete_org(&self, id: OrgId) -> Result<()>;

    // ── Department ──────────────────────────────────────────────────
    fn insert_dept(&self, org_id: OrgId, name: String) -> Result<Department>;
    fn get_dept(&self, id: DeptId) -> Result<Department>;
    fn list_depts_in_org(&self, org_id: OrgId) -> Vec<Department>;
    fn delete_dept(&self, id: DeptId) -> Result<()>;

    // ── Workspace ───────────────────────────────────────────────────
    fn insert_workspace(&self, dept_id: DeptId, name: String) -> Result<Workspace>;
    fn get_workspace(&self, id: WorkspaceId) -> Result<Workspace>;
    fn list_workspaces_in_dept(&self, dept_id: DeptId) -> Vec<Workspace>;
    fn delete_workspace(&self, id: WorkspaceId) -> Result<()>;

    // ── Document ────────────────────────────────────────────────────
    fn insert_document(&self, doc: Document) -> Result<Document>;
    fn get_document(&self, id: DocId) -> Result<Document>;
    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document>;
    fn delete_document(&self, id: DocId) -> Result<()>;

    // ── User ────────────────────────────────────────────────────────
    fn insert_user(&self, email: String, name: String, password_hash: String) -> Result<User>;
    fn get_user_by_email(&self, email: &str) -> Result<UserRecord>;
    fn get_user(&self, id: UserId) -> Result<User>;

    // ── Permissions ─────────────────────────────────────────────────
    fn add_permission(&self, perm: UserPermission);
    fn upsert_permission(&self, perm: UserPermission) -> bool;
    fn list_permissions_for_org(&self, org_id: OrgId) -> Vec<UserPermission>;
    fn remove_permission(&self, user_id: UserId, scope: &PermissionScope) -> Result<()>;
    fn count_org_owners(&self, org_id: OrgId) -> usize;
    fn get_user_role_for_org(&self, user_id: UserId, org_id: OrgId) -> Option<Role>;
    fn get_user_workspace_ids(&self, user_id: UserId) -> Vec<WorkspaceId>;

    // ── Traversal ───────────────────────────────────────────────────
    fn org_id_for_workspace(&self, workspace_id: WorkspaceId) -> Result<OrgId>;

    // ── Cascade helpers ─────────────────────────────────────────────
    fn workspace_ids_in_dept(&self, dept_id: DeptId) -> Vec<WorkspaceId>;
    fn dept_ids_in_org(&self, org_id: OrgId) -> Vec<DeptId>;
    fn doc_ids_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<DocId>;
    fn cascade_delete_workspace_docs(&self, workspace_id: WorkspaceId) -> Vec<DocId>;
    fn cascade_delete_workspace(&self, ws_id: WorkspaceId) -> Result<Vec<DocId>>;
    fn cascade_delete_dept(&self, dept_id: DeptId) -> Result<Vec<DocId>>;
    fn cascade_delete_org(&self, org_id: OrgId) -> Result<Vec<DocId>>;
}

/// Factory function to create the appropriate KM store.
pub fn create_km_store(db_url: &str) -> std::sync::Arc<dyn KmStoreTrait> {
    if db_url.is_empty() || db_url.starts_with("postgres") {
        std::sync::Arc::new(memory::MemoryKmStore::new())
    } else {
        std::sync::Arc::new(sqlite::SqliteKmStore::new(db_url).expect("SQLite init failed"))
    }
}
