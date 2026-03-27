pub mod memory;
pub mod postgres;
pub mod sqlite;

use std::collections::HashMap;

use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, DocStatus, Document, IdentityProvider, Organization, PermissionScope, User,
    UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{
    ApiKeyId, ConnectorId, DeptId, DocId, IdpId, OrgId, UserId, WorkspaceId,
};

type Result<T> = std::result::Result<T, ThaiRagError>;

// ── Scoped Settings ──────────────────────────────────────────────────

/// Hierarchical scope for settings with inheritance:
/// Workspace → Dept → Org → Global.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SettingsScope {
    Global,
    Org(OrgId),
    Dept {
        org_id: OrgId,
        dept_id: DeptId,
    },
    Workspace {
        org_id: OrgId,
        dept_id: DeptId,
        workspace_id: WorkspaceId,
    },
}

impl SettingsScope {
    /// Returns the inheritance chain from most-specific to global.
    /// Each element is `(scope_type, scope_id)` matching DB columns.
    pub fn inheritance_chain(&self) -> Vec<(&str, String)> {
        match self {
            SettingsScope::Global => vec![("global", String::new())],
            SettingsScope::Org(org_id) => {
                vec![("org", org_id.0.to_string()), ("global", String::new())]
            }
            SettingsScope::Dept { org_id, dept_id } => vec![
                ("dept", dept_id.0.to_string()),
                ("org", org_id.0.to_string()),
                ("global", String::new()),
            ],
            SettingsScope::Workspace {
                org_id,
                dept_id,
                workspace_id,
            } => vec![
                ("workspace", workspace_id.0.to_string()),
                ("dept", dept_id.0.to_string()),
                ("org", org_id.0.to_string()),
                ("global", String::new()),
            ],
        }
    }

    /// Returns `(scope_type, scope_id)` for the current level only (no parents).
    pub fn as_pair(&self) -> (&str, String) {
        match self {
            SettingsScope::Global => ("global", String::new()),
            SettingsScope::Org(org_id) => ("org", org_id.0.to_string()),
            SettingsScope::Dept { dept_id, .. } => ("dept", dept_id.0.to_string()),
            SettingsScope::Workspace { workspace_id, .. } => {
                ("workspace", workspace_id.0.to_string())
            }
        }
    }
}

/// Resolve a single setting by walking the inheritance chain (most-specific first).
pub fn resolve_setting(
    store: &dyn KmStoreTrait,
    key: &str,
    scope: &SettingsScope,
) -> Option<String> {
    for (scope_type, scope_id) in scope.inheritance_chain() {
        if let Some(val) = store.get_scoped_setting(key, scope_type, &scope_id) {
            return Some(val);
        }
    }
    None
}

/// Batch-resolve all settings by merging from global (least specific) up to the
/// most specific scope level. At most 4 DB queries regardless of key count.
pub fn resolve_all_settings(
    store: &dyn KmStoreTrait,
    scope: &SettingsScope,
) -> HashMap<String, String> {
    let chain = scope.inheritance_chain();
    let mut merged = HashMap::new();
    // Walk from global → most-specific, so more-specific values overwrite
    for (scope_type, scope_id) in chain.into_iter().rev() {
        for (key, value) in store.list_scoped_settings(scope_type, &scope_id) {
            merged.insert(key, value);
        }
    }
    merged
}

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

#[derive(Debug, Clone)]
pub struct VaultKeyRow {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub encrypted_key: String,
    pub key_prefix: String,
    pub key_suffix: String,
    pub base_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct LlmProfileRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub vault_key_id: Option<String>,
    pub max_tokens: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

// ── API Key (M2M Auth) ──────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiKeyRow {
    pub id: ApiKeyId,
    pub name: String,
    /// SHA-256 hex hash of the raw key.
    pub key_hash: String,
    /// Prefix of raw key for display (e.g. "trag_abc1...").
    pub key_prefix: String,
    pub user_id: UserId,
    pub role: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub is_active: bool,
}

// ── Inference Log Types ──────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InferenceLogEntry {
    pub id: String,
    pub timestamp: String,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub org_id: Option<String>,
    pub dept_id: Option<String>,
    pub session_id: Option<String>,
    pub response_id: String,
    // Query
    pub query_text: String,
    pub detected_language: Option<String>,
    pub intent: Option<String>,
    pub complexity: Option<String>,
    // Model
    pub llm_kind: String,
    pub llm_model: String,
    pub settings_scope: String,
    // Tokens
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    // Timing
    pub total_ms: u64,
    pub search_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    // Search
    pub chunks_retrieved: Option<u32>,
    pub avg_chunk_score: Option<f32>,
    pub self_rag_decision: Option<String>,
    pub self_rag_confidence: Option<f32>,
    // Quality
    pub quality_guard_pass: Option<bool>,
    pub relevance_score: Option<f32>,
    pub hallucination_score: Option<f32>,
    pub completeness_score: Option<f32>,
    // Pipeline
    pub pipeline_route: Option<String>,
    pub agents_used: String,
    // Result
    pub status: String,
    pub error_message: Option<String>,
    pub response_length: u32,
    // Feedback (updated later)
    pub feedback_score: Option<i8>,
}

#[derive(Debug, Clone, Default)]
pub struct InferenceLogFilter {
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub from_timestamp: Option<String>,
    pub to_timestamp: Option<String>,
    pub status: Option<String>,
    pub llm_model: Option<String>,
    pub intent: Option<String>,
    pub response_id: Option<String>,
    pub session_id: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InferenceLogListResponse {
    pub entries: Vec<InferenceLogEntry>,
    pub total: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InferenceStats {
    pub total_requests: u64,
    pub avg_total_ms: f64,
    pub avg_search_ms: f64,
    pub avg_generation_ms: f64,
    pub avg_relevance_score: f64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub success_rate: f64,
    pub quality_pass_rate: f64,
    pub feedback_positive_rate: f64,
    pub by_model: Vec<ModelStats>,
    pub by_workspace: Vec<WorkspaceStats>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelStats {
    pub model: String,
    pub count: u64,
    pub avg_ms: f64,
    pub avg_quality: f64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceStats {
    pub workspace_id: String,
    pub count: u64,
    pub avg_ms: f64,
    pub total_tokens: u64,
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
    fn set_user_disabled(&self, id: UserId, disabled: bool) -> Result<User>;

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
    /// Get a global-scope setting (backward-compatible shortcut).
    fn get_setting(&self, key: &str) -> Option<String>;
    /// Set a global-scope setting (backward-compatible shortcut).
    fn set_setting(&self, key: &str, value: &str);
    /// Delete a global-scope setting (backward-compatible shortcut).
    fn delete_setting(&self, key: &str);
    /// List all global-scope settings, excluding internal keys.
    fn list_all_settings(&self) -> Vec<(String, String)>;

    // ── Scoped Settings ───────────────────────────────────────────────
    /// Get a setting at a specific scope level.
    fn get_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str) -> Option<String>;
    /// Set a setting at a specific scope level.
    fn set_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str, value: &str);
    /// Delete a setting at a specific scope level.
    fn delete_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str);
    /// List all settings at a specific scope level.
    fn list_scoped_settings(&self, scope_type: &str, scope_id: &str) -> Vec<(String, String)>;
    /// List which keys have overrides at a specific scope level.
    fn list_override_keys(&self, scope_type: &str, scope_id: &str) -> Vec<String>;
    /// Delete all settings at a specific scope level.
    fn delete_all_scoped_settings(&self, scope_type: &str, scope_id: &str);

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

    // ── API Key Vault ───────────────────────────────────────────────
    fn list_vault_keys(&self) -> Vec<VaultKeyRow>;
    fn get_vault_key(&self, id: &str) -> Option<VaultKeyRow>;
    fn upsert_vault_key(&self, row: &VaultKeyRow);
    fn delete_vault_key(&self, id: &str);

    // ── LLM Profiles ────────────────────────────────────────────────
    fn list_llm_profiles(&self) -> Vec<LlmProfileRow>;
    fn get_llm_profile(&self, id: &str) -> Option<LlmProfileRow>;
    fn upsert_llm_profile(&self, row: &LlmProfileRow);
    fn delete_llm_profile(&self, id: &str);

    // ── API Keys (M2M Auth) ──────────────────────────────────────────
    fn create_api_key(
        &self,
        user_id: UserId,
        name: String,
        key_hash: String,
        key_prefix: String,
        role: String,
    ) -> Result<ApiKeyRow>;
    fn get_api_key_by_hash(&self, key_hash: &str) -> Option<ApiKeyRow>;
    fn list_api_keys(&self, user_id: UserId) -> Vec<ApiKeyRow>;
    fn revoke_api_key(&self, key_id: ApiKeyId) -> Result<()>;
    fn touch_api_key(&self, key_id: ApiKeyId);

    // ── Inference Logs ────────────────────────────────────────────────
    fn insert_inference_log(&self, entry: &InferenceLogEntry);
    fn list_inference_logs(&self, filter: &InferenceLogFilter) -> Vec<InferenceLogEntry>;
    fn get_inference_stats(&self, filter: &InferenceLogFilter) -> InferenceStats;
    fn update_inference_log_feedback(&self, response_id: &str, score: i8);
    fn delete_inference_logs(&self, filter: &InferenceLogFilter) -> u64;
    fn count_inference_logs(&self, filter: &InferenceLogFilter) -> u64;
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
