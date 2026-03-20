pub mod memory;
pub mod postgres;
pub mod sqlite;

use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, DocStatus, Document, IdentityProvider, Organization, PermissionScope, User,
    UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{ConnectorId, DeptId, DocId, IdpId, OrgId, UserId, WorkspaceId};

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
    fn list_workspaces_all(&self) -> Vec<Workspace>;
    fn delete_workspace(&self, id: WorkspaceId) -> Result<()>;

    // ── Document ────────────────────────────────────────────────────
    fn insert_document(&self, doc: Document) -> Result<Document>;
    fn get_document(&self, id: DocId) -> Result<Document>;
    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document>;
    fn update_document_status(
        &self,
        id: DocId,
        status: DocStatus,
        chunk_count: i64,
        error_message: Option<String>,
    ) -> Result<()>;
    fn update_document_step(&self, id: DocId, step: Option<String>) -> Result<()>;
    fn delete_document(&self, id: DocId) -> Result<()>;
    /// Store original file bytes and converted markdown for a document.
    fn save_document_blob(
        &self,
        doc_id: DocId,
        original_bytes: Option<Vec<u8>>,
        converted_text: Option<String>,
        image_count: i32,
        table_count: i32,
    ) -> Result<()>;
    /// Retrieve converted markdown text for a document.
    fn get_document_content(&self, doc_id: DocId) -> Result<Option<String>>;
    /// Retrieve original file bytes for a document.
    fn get_document_file(&self, doc_id: DocId) -> Result<Option<Vec<u8>>>;
    /// Get image and table counts for a document.
    fn get_document_blob_stats(&self, doc_id: DocId) -> Result<(i32, i32)>;

    // ── Document Chunks (for Tantivy rebuild) ──────────────────────
    fn save_chunks(&self, chunks: &[thairag_core::types::DocumentChunk]) -> Result<()>;
    fn load_all_chunks(&self) -> Vec<thairag_core::types::DocumentChunk>;
    fn delete_chunks_by_doc(&self, doc_id: DocId) -> Result<()>;

    // ── User ────────────────────────────────────────────────────────
    fn insert_user(&self, email: String, name: String, password_hash: String) -> Result<User>;
    fn upsert_user_by_email(
        &self,
        email: String,
        name: String,
        password_hash: String,
        is_super_admin: bool,
        role: String,
    ) -> Result<User>;
    fn delete_user(&self, id: UserId) -> Result<()>;
    fn get_user_by_email(&self, email: &str) -> Result<UserRecord>;
    fn get_user(&self, id: UserId) -> Result<User>;
    fn list_users(&self) -> Vec<User>;

    // ── Identity Providers ──────────────────────────────────────────
    fn list_identity_providers(&self) -> Vec<IdentityProvider>;
    fn list_enabled_identity_providers(&self) -> Vec<IdentityProvider>;
    fn get_identity_provider(&self, id: IdpId) -> Result<IdentityProvider>;
    fn insert_identity_provider(
        &self,
        name: String,
        provider_type: String,
        enabled: bool,
        config: serde_json::Value,
    ) -> Result<IdentityProvider>;
    fn update_identity_provider(
        &self,
        id: IdpId,
        name: String,
        provider_type: String,
        enabled: bool,
        config: serde_json::Value,
    ) -> Result<IdentityProvider>;
    fn delete_identity_provider(&self, id: IdpId) -> Result<()>;

    // ── Permissions ─────────────────────────────────────────────────
    fn add_permission(&self, perm: UserPermission);
    fn upsert_permission(&self, perm: UserPermission) -> bool;
    fn list_permissions_for_org(&self, org_id: OrgId) -> Vec<UserPermission>;
    fn remove_permission(&self, user_id: UserId, scope: &PermissionScope) -> Result<()>;
    fn count_org_owners(&self, org_id: OrgId) -> usize;
    fn get_user_role_for_org(&self, user_id: UserId, org_id: OrgId) -> Option<Role>;
    /// Role from org-level + matching dept-level permissions (inherits from parent).
    fn get_user_role_for_dept(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
    ) -> Option<Role>;
    /// Role from org + dept + matching workspace-level permissions (inherits from parents).
    fn get_user_role_for_workspace(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
        workspace_id: WorkspaceId,
    ) -> Option<Role>;
    /// All permissions for a user across all scopes.
    fn list_user_permissions(&self, user_id: UserId) -> Vec<UserPermission>;
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

    // ── Settings (key-value store) ───────────────────────────────────
    fn get_setting(&self, key: &str) -> Option<String>;
    fn set_setting(&self, key: &str, value: &str);
    fn delete_setting(&self, key: &str);

    // ── MCP Connectors ───────────────────────────────────────────────
    fn insert_connector(
        &self,
        config: thairag_core::types::McpConnectorConfig,
    ) -> Result<thairag_core::types::McpConnectorConfig>;
    fn get_connector(&self, id: ConnectorId) -> Result<thairag_core::types::McpConnectorConfig>;
    fn list_connectors(&self) -> Vec<thairag_core::types::McpConnectorConfig>;
    fn list_connectors_for_workspace(
        &self,
        ws_id: WorkspaceId,
    ) -> Vec<thairag_core::types::McpConnectorConfig>;
    fn update_connector(&self, config: thairag_core::types::McpConnectorConfig) -> Result<()>;
    fn delete_connector(&self, id: ConnectorId) -> Result<()>;
    fn update_connector_status(
        &self,
        id: ConnectorId,
        status: thairag_core::types::ConnectorStatus,
    ) -> Result<()>;

    // ── MCP Sync State ───────────────────────────────────────────────
    fn get_sync_state(
        &self,
        connector_id: ConnectorId,
        resource_uri: &str,
    ) -> Option<thairag_core::types::SyncState>;
    fn upsert_sync_state(&self, state: thairag_core::types::SyncState) -> Result<()>;
    fn list_sync_states(&self, connector_id: ConnectorId) -> Vec<thairag_core::types::SyncState>;
    fn delete_sync_states(&self, connector_id: ConnectorId) -> Result<()>;

    // ── MCP Sync Runs ────────────────────────────────────────────────
    fn insert_sync_run(&self, run: thairag_core::types::SyncRun) -> Result<()>;
    fn update_sync_run(&self, run: thairag_core::types::SyncRun) -> Result<()>;
    fn list_sync_runs(
        &self,
        connector_id: ConnectorId,
        limit: usize,
    ) -> Vec<thairag_core::types::SyncRun>;
    fn get_latest_sync_run(
        &self,
        connector_id: ConnectorId,
    ) -> Option<thairag_core::types::SyncRun>;
}

/// Factory function to create the appropriate KM store.
pub fn create_km_store(db_url: &str, max_connections: u32) -> std::sync::Arc<dyn KmStoreTrait> {
    if db_url.is_empty() {
        std::sync::Arc::new(memory::MemoryKmStore::new())
    } else if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
        let store = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(postgres::PostgresKmStore::new(db_url, max_connections))
        })
        .expect("Postgres init failed");
        std::sync::Arc::new(store)
    } else {
        std::sync::Arc::new(sqlite::SqliteKmStore::new(db_url).expect("SQLite init failed"))
    }
}
