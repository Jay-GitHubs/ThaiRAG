use std::future::Future;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, DocStatus, Document, IdentityProvider, Organization, PermissionScope, User,
    UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{
    ApiKeyId, ConnectorId, ConnectorStatus, DeptId, DocId, IdpId, McpConnectorConfig, McpTransport,
    OrgId, SyncMode, SyncRun, SyncRunId, SyncRunStatus, SyncState, UserId, WorkspaceId,
};
use uuid::Uuid;

use super::{KmStoreTrait, UserRecord};

type Result<T> = std::result::Result<T, ThaiRagError>;

/// Bridge async sqlx into the sync `KmStoreTrait` using `block_in_place`.
fn block_on<F: Future>(f: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
}

pub struct PostgresKmStore {
    pool: PgPool,
}

impl PostgresKmStore {
    pub async fn new(
        db_url: &str,
        max_connections: u32,
    ) -> std::result::Result<Self, ThaiRagError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(db_url)
            .await
            .map_err(|e| ThaiRagError::Config(format!("Postgres connect failed: {e}")))?;

        let schema = include_str!("postgres_schema.sql");
        sqlx::raw_sql(schema)
            .execute(&pool)
            .await
            .map_err(|e| ThaiRagError::Config(format!("Postgres schema failed: {e}")))?;

        Ok(Self { pool })
    }
}

// ── Helper functions ────────────────────────────────────────────────

fn scope_to_parts(scope: &PermissionScope) -> (&str, String, String, String) {
    match scope {
        PermissionScope::Org { org_id } => {
            ("org", org_id.0.to_string(), String::new(), String::new())
        }
        PermissionScope::Dept { org_id, dept_id } => (
            "dept",
            org_id.0.to_string(),
            dept_id.0.to_string(),
            String::new(),
        ),
        PermissionScope::Workspace {
            org_id,
            dept_id,
            workspace_id,
        } => (
            "workspace",
            org_id.0.to_string(),
            dept_id.0.to_string(),
            workspace_id.0.to_string(),
        ),
    }
}

fn parts_to_scope(level: &str, org_id: &str, dept_id: &str, ws_id: &str) -> PermissionScope {
    match level {
        "dept" => PermissionScope::Dept {
            org_id: OrgId(org_id.parse().unwrap_or_default()),
            dept_id: DeptId(dept_id.parse().unwrap_or_default()),
        },
        "workspace" => PermissionScope::Workspace {
            org_id: OrgId(org_id.parse().unwrap_or_default()),
            dept_id: DeptId(dept_id.parse().unwrap_or_default()),
            workspace_id: WorkspaceId(ws_id.parse().unwrap_or_default()),
        },
        _ => PermissionScope::Org {
            org_id: OrgId(org_id.parse().unwrap_or_default()),
        },
    }
}

fn parse_role(s: &str) -> Role {
    match s {
        "owner" => Role::Owner,
        "admin" => Role::Admin,
        "editor" => Role::Editor,
        _ => Role::Viewer,
    }
}

fn role_str(r: &Role) -> &'static str {
    match r {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
    }
}

fn pg_transport_str(t: &McpTransport) -> &'static str {
    match t {
        McpTransport::Stdio => "stdio",
        McpTransport::Sse => "sse",
    }
}

fn pg_parse_transport(s: &str) -> McpTransport {
    match s {
        "sse" => McpTransport::Sse,
        _ => McpTransport::Stdio,
    }
}

fn pg_connector_status_str(s: &ConnectorStatus) -> &'static str {
    match s {
        ConnectorStatus::Active => "active",
        ConnectorStatus::Paused => "paused",
        ConnectorStatus::Error => "error",
        ConnectorStatus::Syncing => "syncing",
    }
}

fn pg_parse_connector_status(s: &str) -> ConnectorStatus {
    match s {
        "paused" => ConnectorStatus::Paused,
        "error" => ConnectorStatus::Error,
        "syncing" => ConnectorStatus::Syncing,
        _ => ConnectorStatus::Active,
    }
}

fn pg_sync_mode_str(m: &SyncMode) -> &'static str {
    match m {
        SyncMode::OnDemand => "on_demand",
        SyncMode::Scheduled => "scheduled",
    }
}

fn pg_parse_sync_mode(s: &str) -> SyncMode {
    match s {
        "scheduled" => SyncMode::Scheduled,
        _ => SyncMode::OnDemand,
    }
}

fn pg_sync_run_status_str(s: &SyncRunStatus) -> &'static str {
    match s {
        SyncRunStatus::Running => "running",
        SyncRunStatus::Completed => "completed",
        SyncRunStatus::Failed => "failed",
        SyncRunStatus::Cancelled => "cancelled",
    }
}

fn pg_parse_sync_run_status(s: &str) -> SyncRunStatus {
    match s {
        "completed" => SyncRunStatus::Completed,
        "failed" => SyncRunStatus::Failed,
        "cancelled" => SyncRunStatus::Cancelled,
        _ => SyncRunStatus::Running,
    }
}

fn pg_row_to_connector(row: &sqlx::postgres::PgRow) -> McpConnectorConfig {
    let id: Uuid = row.get("id");
    let name: String = row.get("name");
    let description: String = row.get("description");
    let transport: String = row.get("transport");
    let command: Option<String> = row.get("command");
    let args: String = row.get("args");
    let env: String = row.get("env");
    let url: Option<String> = row.get("url");
    let headers: String = row.get("headers");
    let ws_id: Uuid = row.get("workspace_id");
    let sync_mode: String = row.get("sync_mode");
    let schedule_cron: Option<String> = row.get("schedule_cron");
    let resource_filters: String = row.get("resource_filters");
    let max_items: Option<i32> = row.get("max_items_per_sync");
    let tool_calls: String = row.get("tool_calls");
    let webhook_url: Option<String> = row.get("webhook_url");
    let webhook_secret: Option<String> = row.get("webhook_secret");
    let status: String = row.get("status");
    let ca: DateTime<Utc> = row.get("created_at");
    let ua: DateTime<Utc> = row.get("updated_at");
    McpConnectorConfig {
        id: ConnectorId(id),
        name,
        description,
        transport: pg_parse_transport(&transport),
        command,
        args: serde_json::from_str(&args).unwrap_or_default(),
        env: serde_json::from_str(&env).unwrap_or_default(),
        url,
        headers: serde_json::from_str(&headers).unwrap_or_default(),
        workspace_id: WorkspaceId(ws_id),
        sync_mode: pg_parse_sync_mode(&sync_mode),
        schedule_cron,
        resource_filters: serde_json::from_str(&resource_filters).unwrap_or_default(),
        max_items_per_sync: max_items.map(|v| v as usize),
        tool_calls: serde_json::from_str(&tool_calls).unwrap_or_default(),
        webhook_url,
        webhook_secret,
        status: pg_parse_connector_status(&status),
        created_at: ca,
        updated_at: ua,
    }
}

#[allow(clippy::too_many_arguments)]
fn pg_row_to_sync_run(
    id: Uuid,
    cid: Uuid,
    started: DateTime<Utc>,
    completed: Option<DateTime<Utc>>,
    status: String,
    disc: i32,
    crea: i32,
    upd: i32,
    skip: i32,
    fail: i32,
    err: Option<String>,
) -> SyncRun {
    SyncRun {
        id: SyncRunId(id),
        connector_id: ConnectorId(cid),
        started_at: started,
        completed_at: completed,
        status: pg_parse_sync_run_status(&status),
        items_discovered: disc as usize,
        items_created: crea as usize,
        items_updated: upd as usize,
        items_skipped: skip as usize,
        items_failed: fail as usize,
        error_message: err,
    }
}

// ── KmStoreTrait implementation ─────────────────────────────────────

impl KmStoreTrait for PostgresKmStore {
    // ── Organization ────────────────────────────────────────────────

    fn insert_org(&self, name: String) -> Result<Organization> {
        let now = Utc::now();
        let org = Organization {
            id: OrgId::new(),
            name,
            created_at: now,
            updated_at: now,
        };
        block_on(sqlx::query(
            "INSERT INTO organizations (id, name, created_at, updated_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(org.id.0)
        .bind(&org.name)
        .bind(now)
        .bind(now)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert org: {e}")))?;
        Ok(org)
    }

    fn get_org(&self, id: OrgId) -> Result<Organization> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, created_at, updated_at FROM organizations WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, name, ca, ua)| Organization {
            id: OrgId(id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("Organization {id} not found")))
    }

    fn list_orgs(&self) -> Vec<Organization> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, created_at, updated_at FROM organizations",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name, ca, ua)| Organization {
            id: OrgId(id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn delete_org(&self, id: OrgId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete org: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Organization {id} not found"
            )));
        }
        Ok(())
    }

    // ── Department ──────────────────────────────────────────────────

    fn insert_dept(&self, org_id: OrgId, name: String) -> Result<Department> {
        self.get_org(org_id)?;
        let now = Utc::now();
        let dept = Department {
            id: DeptId::new(),
            org_id,
            name,
            created_at: now,
            updated_at: now,
        };
        block_on(sqlx::query(
            "INSERT INTO departments (id, org_id, name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(dept.id.0)
        .bind(org_id.0)
        .bind(&dept.name)
        .bind(now)
        .bind(now)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert dept: {e}")))?;
        Ok(dept)
    }

    fn get_dept(&self, id: DeptId) -> Result<Department> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, org_id, name, created_at, updated_at FROM departments WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, org_id, name, ca, ua)| Department {
            id: DeptId(id),
            org_id: OrgId(org_id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("Department {id} not found")))
    }

    fn list_depts_in_org(&self, org_id: OrgId) -> Vec<Department> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, org_id, name, created_at, updated_at FROM departments WHERE org_id = $1",
            )
            .bind(org_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, org_id, name, ca, ua)| Department {
            id: DeptId(id),
            org_id: OrgId(org_id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn delete_dept(&self, id: DeptId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM departments WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete dept: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Department {id} not found")));
        }
        Ok(())
    }

    // ── Workspace ───────────────────────────────────────────────────

    fn insert_workspace(&self, dept_id: DeptId, name: String) -> Result<Workspace> {
        self.get_dept(dept_id)?;
        let now = Utc::now();
        let ws = Workspace {
            id: WorkspaceId::new(),
            dept_id,
            name,
            created_at: now,
            updated_at: now,
        };
        block_on(sqlx::query(
            "INSERT INTO workspaces (id, dept_id, name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(ws.id.0)
        .bind(dept_id.0)
        .bind(&ws.name)
        .bind(now)
        .bind(now)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert workspace: {e}")))?;
        Ok(ws)
    }

    fn get_workspace(&self, id: WorkspaceId) -> Result<Workspace> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, dept_id, name, created_at, updated_at FROM workspaces WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, dept_id, name, ca, ua)| Workspace {
            id: WorkspaceId(id),
            dept_id: DeptId(dept_id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("Workspace {id} not found")))
    }

    fn list_workspaces_in_dept(&self, dept_id: DeptId) -> Vec<Workspace> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, dept_id, name, created_at, updated_at FROM workspaces WHERE dept_id = $1",
            )
            .bind(dept_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, dept_id, name, ca, ua)| Workspace {
            id: WorkspaceId(id),
            dept_id: DeptId(dept_id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn list_workspaces_all(&self) -> Vec<Workspace> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, dept_id, name, created_at, updated_at FROM workspaces",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, dept_id, name, ca, ua)| Workspace {
            id: WorkspaceId(id),
            dept_id: DeptId(dept_id),
            name,
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM workspaces WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete workspace: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Workspace {id} not found")));
        }
        Ok(())
    }

    // ── Document ────────────────────────────────────────────────────

    fn insert_document(&self, doc: Document) -> Result<Document> {
        self.get_workspace(doc.workspace_id)?;
        block_on(sqlx::query(
            "INSERT INTO documents (id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
        )
        .bind(doc.id.0)
        .bind(doc.workspace_id.0)
        .bind(&doc.title)
        .bind(&doc.mime_type)
        .bind(doc.size_bytes)
        .bind(doc.status.to_string())
        .bind(doc.chunk_count)
        .bind(&doc.error_message)
        .bind(&doc.processing_step)
        .bind(doc.version)
        .bind(&doc.content_hash)
        .bind(&doc.source_url)
        .bind(&doc.refresh_schedule)
        .bind(doc.last_refreshed_at)
        .bind(doc.created_at)
        .bind(doc.updated_at)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert document: {e}")))?;
        Ok(doc)
    }

    fn get_document(&self, id: DocId) -> Result<Document> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, String, i64, String, i32, Option<String>, Option<String>, i32, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, COALESCE(version, 1), content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at FROM documents WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, ws_id, title, mime, size, status, chunks, err_msg, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, ca, ua)| Document {
            id: DocId(id),
            workspace_id: WorkspaceId(ws_id),
            title,
            mime_type: mime,
            size_bytes: size,
            status: DocStatus::from_str_lossy(&status),
            chunk_count: chunks as i64,
            error_message: err_msg,
            processing_step,
            version,
            content_hash,
            source_url,
            refresh_schedule,
            last_refreshed_at,
            created_at: ca,
            updated_at: ua,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, String, i64, String, i32, Option<String>, Option<String>, i32, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, COALESCE(version, 1), content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at FROM documents WHERE workspace_id = $1",
            )
            .bind(workspace_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, ws_id, title, mime, size, status, chunks, err_msg, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, ca, ua)| Document {
            id: DocId(id),
            workspace_id: WorkspaceId(ws_id),
            title,
            mime_type: mime,
            size_bytes: size,
            status: DocStatus::from_str_lossy(&status),
            chunk_count: chunks as i64,
            error_message: err_msg,
            processing_step,
            version,
            content_hash,
            source_url,
            refresh_schedule,
            last_refreshed_at,
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn update_document_status(
        &self,
        id: DocId,
        status: DocStatus,
        chunk_count: i64,
        error_message: Option<String>,
    ) -> Result<()> {
        let result = block_on(
            sqlx::query("UPDATE documents SET status = $1, chunk_count = $2, error_message = $3, updated_at = $4 WHERE id = $5")
                .bind(status.to_string())
                .bind(chunk_count)
                .bind(&error_message)
                .bind(Utc::now())
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update document status: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn update_document_step(&self, id: DocId, step: Option<String>) -> Result<()> {
        let result = block_on(
            sqlx::query("UPDATE documents SET processing_step = $1, updated_at = $2 WHERE id = $3")
                .bind(&step)
                .bind(Utc::now())
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update document step: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn delete_document(&self, id: DocId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM documents WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete document: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn save_document_blob(
        &self,
        doc_id: DocId,
        original_bytes: Option<Vec<u8>>,
        converted_text: Option<String>,
        image_count: i32,
        table_count: i32,
    ) -> Result<()> {
        block_on(
            sqlx::query(
                "INSERT INTO document_blobs (doc_id, original_bytes, converted_text, image_count, table_count, created_at)
                 VALUES ($1, $2, $3, $4, $5, NOW())
                 ON CONFLICT (doc_id) DO UPDATE SET
                   original_bytes = COALESCE($2, document_blobs.original_bytes),
                   converted_text = COALESCE($3, document_blobs.converted_text),
                   image_count = $4,
                   table_count = $5"
            )
            .bind(doc_id.0)
            .bind(&original_bytes)
            .bind(&converted_text)
            .bind(image_count)
            .bind(table_count)
            .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres save blob: {e}")))?;
        Ok(())
    }

    fn get_document_content(&self, doc_id: DocId) -> Result<Option<String>> {
        let row = block_on(
            sqlx::query_as::<_, (Option<String>,)>(
                "SELECT converted_text FROM document_blobs WHERE doc_id = $1",
            )
            .bind(doc_id.0)
            .fetch_optional(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres get content: {e}")))?;
        Ok(row.and_then(|(t,)| t))
    }

    fn get_document_file(&self, doc_id: DocId) -> Result<Option<Vec<u8>>> {
        let row = block_on(
            sqlx::query_as::<_, (Option<Vec<u8>>,)>(
                "SELECT original_bytes FROM document_blobs WHERE doc_id = $1",
            )
            .bind(doc_id.0)
            .fetch_optional(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres get file: {e}")))?;
        Ok(row.and_then(|(b,)| b))
    }

    fn get_document_blob_stats(&self, doc_id: DocId) -> Result<(i32, i32)> {
        let row = block_on(
            sqlx::query_as::<_, (i32, i32)>(
                "SELECT image_count, table_count FROM document_blobs WHERE doc_id = $1",
            )
            .bind(doc_id.0)
            .fetch_optional(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres get blob stats: {e}")))?;
        Ok(row.unwrap_or((0, 0)))
    }

    fn update_document_version_info(
        &self,
        id: DocId,
        version: i32,
        content_hash: Option<String>,
    ) -> Result<()> {
        let result = block_on(
            sqlx::query("UPDATE documents SET version = $1, content_hash = $2, updated_at = $3 WHERE id = $4")
                .bind(version)
                .bind(&content_hash)
                .bind(Utc::now())
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update document version info: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    // ── Document Versioning ─────────────────────────────────────────

    fn save_document_version(
        &self,
        doc_id: DocId,
        title: &str,
        content: Option<&str>,
        content_hash: &str,
        mime_type: &str,
        size_bytes: i64,
        created_by: Option<UserId>,
    ) -> Result<super::DocumentVersion> {
        let next_version: i32 = block_on(
            sqlx::query_as::<_, (i32,)>(
                "SELECT COALESCE(MAX(version_number), 0) + 1 FROM document_versions WHERE doc_id = $1",
            )
            .bind(doc_id.0)
            .fetch_one(&self.pool),
        )
        .map(|(v,)| v)
        .unwrap_or(1);

        let id = Uuid::new_v4();
        let now = Utc::now();
        block_on(
            sqlx::query(
                "INSERT INTO document_versions (id, doc_id, version_number, title, content, content_hash, mime_type, size_bytes, created_at, created_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(id)
            .bind(doc_id.0)
            .bind(next_version)
            .bind(title)
            .bind(content)
            .bind(content_hash)
            .bind(mime_type)
            .bind(size_bytes)
            .bind(now)
            .bind(created_by.map(|u| u.0))
            .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres save document version: {e}")))?;

        Ok(super::DocumentVersion {
            id: id.to_string(),
            doc_id,
            version_number: next_version,
            title: title.to_string(),
            content: content.map(|s| s.to_string()),
            content_hash: content_hash.to_string(),
            mime_type: mime_type.to_string(),
            size_bytes,
            created_at: now.to_rfc3339(),
            created_by,
        })
    }

    fn list_document_versions(&self, doc_id: DocId) -> Vec<super::DocumentVersion> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, i32, String, Option<String>, String, String, i64, DateTime<Utc>, Option<Uuid>)>(
                "SELECT id, doc_id, version_number, title, content, content_hash, mime_type, size_bytes, created_at, created_by
                 FROM document_versions WHERE doc_id = $1 ORDER BY version_number DESC",
            )
            .bind(doc_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, did, vn, title, content, hash, mime, size, ca, cb)| super::DocumentVersion {
            id: id.to_string(),
            doc_id: DocId(did),
            version_number: vn,
            title,
            content,
            content_hash: hash,
            mime_type: mime,
            size_bytes: size,
            created_at: ca.to_rfc3339(),
            created_by: cb.map(UserId),
        })
        .collect()
    }

    fn get_document_version(
        &self,
        doc_id: DocId,
        version_number: i32,
    ) -> Option<super::DocumentVersion> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, i32, String, Option<String>, String, String, i64, DateTime<Utc>, Option<Uuid>)>(
                "SELECT id, doc_id, version_number, title, content, content_hash, mime_type, size_bytes, created_at, created_by
                 FROM document_versions WHERE doc_id = $1 AND version_number = $2",
            )
            .bind(doc_id.0)
            .bind(version_number)
            .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
        .map(|(id, did, vn, title, content, hash, mime, size, ca, cb)| super::DocumentVersion {
            id: id.to_string(),
            doc_id: DocId(did),
            version_number: vn,
            title,
            content,
            content_hash: hash,
            mime_type: mime,
            size_bytes: size,
            created_at: ca.to_rfc3339(),
            created_by: cb.map(UserId),
        })
    }

    // ── Document Refresh Schedule ────────────────────────────────

    fn update_document_schedule(
        &self,
        id: DocId,
        source_url: Option<String>,
        refresh_schedule: Option<String>,
    ) -> Result<()> {
        let result = block_on(
            sqlx::query("UPDATE documents SET source_url = $1, refresh_schedule = $2, updated_at = $3 WHERE id = $4")
                .bind(&source_url)
                .bind(&refresh_schedule)
                .bind(Utc::now())
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update document schedule: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn touch_document_refreshed(&self, id: DocId) -> Result<()> {
        let now = Utc::now();
        let result = block_on(
            sqlx::query(
                "UPDATE documents SET last_refreshed_at = $1, updated_at = $2 WHERE id = $3",
            )
            .bind(now)
            .bind(now)
            .bind(id.0)
            .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres touch document refreshed: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn list_documents_due_for_refresh(&self) -> Vec<Document> {
        let all: Vec<Document> = block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, String, i64, String, i32, Option<String>, Option<String>, i32, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, COALESCE(version, 1), content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at
                 FROM documents WHERE status = 'ready' AND source_url IS NOT NULL AND refresh_schedule IS NOT NULL",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, ws_id, title, mime, size, status, chunks, err_msg, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, ca, ua)| Document {
            id: DocId(id),
            workspace_id: WorkspaceId(ws_id),
            title,
            mime_type: mime,
            size_bytes: size,
            status: DocStatus::from_str_lossy(&status),
            chunk_count: chunks as i64,
            error_message: err_msg,
            processing_step,
            version,
            content_hash,
            source_url,
            refresh_schedule,
            last_refreshed_at,
            created_at: ca,
            updated_at: ua,
        })
        .collect();
        let now = Utc::now();
        all.into_iter()
            .filter(|doc| {
                let schedule = match &doc.refresh_schedule {
                    Some(s) => s,
                    None => return false,
                };
                let interval = match super::parse_refresh_interval(schedule) {
                    Some(d) => chrono::Duration::from_std(d).unwrap_or(chrono::Duration::days(1)),
                    None => return false,
                };
                let last = doc.last_refreshed_at.unwrap_or(doc.created_at);
                now - last >= interval
            })
            .collect()
    }

    // ── Document Chunks ────────────────────────────────────────────

    fn save_chunks(&self, chunks: &[thairag_core::types::DocumentChunk]) -> Result<()> {
        for chunk in chunks {
            block_on(
                sqlx::query(
                    "INSERT INTO document_chunks (chunk_id, doc_id, workspace_id, content, chunk_index)
                     VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (chunk_id) DO UPDATE SET
                       content = $4, chunk_index = $5",
                )
                .bind(chunk.chunk_id.0)
                .bind(chunk.doc_id.0)
                .bind(chunk.workspace_id.0)
                .bind(&chunk.content)
                .bind(chunk.chunk_index as i32)
                .execute(&self.pool),
            )
            .map_err(|e| ThaiRagError::Internal(format!("Postgres save chunk: {e}")))?;
        }
        Ok(())
    }

    fn load_all_chunks(&self) -> Vec<thairag_core::types::DocumentChunk> {
        use thairag_core::types::{ChunkId, DocumentChunk, WorkspaceId};
        let rows: Vec<(Uuid, Uuid, Uuid, String, i32)> = block_on(
            sqlx::query_as(
                "SELECT chunk_id, doc_id, workspace_id, content, chunk_index FROM document_chunks",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default();

        rows.into_iter()
            .map(
                |(chunk_id, doc_id, workspace_id, content, chunk_index)| DocumentChunk {
                    chunk_id: ChunkId(chunk_id),
                    doc_id: DocId(doc_id),
                    workspace_id: WorkspaceId(workspace_id),
                    content,
                    chunk_index: chunk_index as usize,
                    embedding: None,
                    metadata: None,
                },
            )
            .collect()
    }

    fn delete_chunks_by_doc(&self, doc_id: DocId) -> Result<()> {
        block_on(
            sqlx::query("DELETE FROM document_chunks WHERE doc_id = $1")
                .bind(doc_id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete chunks: {e}")))?;
        Ok(())
    }

    // ── User ────────────────────────────────────────────────────────

    fn insert_user(&self, email: String, name: String, password_hash: String) -> Result<User> {
        let email_lower = email.to_lowercase();

        let exists: bool = block_on(
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM users WHERE email = $1")
                .bind(&email_lower)
                .fetch_one(&self.pool),
        )
        .map(|(c,)| c > 0)
        .unwrap_or(false);

        if exists {
            return Err(ThaiRagError::Validation(format!(
                "Email {email} is already registered"
            )));
        }

        let user = User {
            id: UserId::new(),
            email: email_lower,
            name,
            auth_provider: "local".into(),
            external_id: None,
            is_super_admin: false,
            role: "viewer".into(),
            disabled: false,
            created_at: Utc::now(),
        };
        block_on(sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, disabled, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(user.id.0)
        .bind(&user.email)
        .bind(&user.name)
        .bind(&password_hash)
        .bind(&user.auth_provider)
        .bind(&user.external_id)
        .bind(user.is_super_admin)
        .bind(&user.role)
        .bind(user.disabled)
        .bind(user.created_at)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert user: {e}")))?;
        Ok(user)
    }

    fn upsert_user_by_email(
        &self,
        email: String,
        name: String,
        password_hash: String,
        is_super_admin: bool,
        role: String,
    ) -> Result<User> {
        let email_lower = email.to_lowercase();

        let existing: Option<Uuid> = block_on(
            sqlx::query_as::<_, (Uuid,)>("SELECT id FROM users WHERE email = $1")
                .bind(&email_lower)
                .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
        .map(|(id,)| id);

        if let Some(id) = existing {
            block_on(sqlx::query(
                "UPDATE users SET name = $1, password_hash = $2, is_super_admin = $3, role = $4 WHERE id = $5",
            )
            .bind(&name)
            .bind(&password_hash)
            .bind(is_super_admin)
            .bind(&role)
            .bind(id)
            .execute(&self.pool))
            .map_err(|e| ThaiRagError::Internal(format!("Postgres upsert user: {e}")))?;
            return self.get_user(UserId(id));
        }

        let user = User {
            id: UserId::new(),
            email: email_lower,
            name,
            auth_provider: "local".into(),
            external_id: None,
            is_super_admin,
            role,
            disabled: false,
            created_at: Utc::now(),
        };
        block_on(sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, disabled, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(user.id.0)
        .bind(&user.email)
        .bind(&user.name)
        .bind(&password_hash)
        .bind(&user.auth_provider)
        .bind(&user.external_id)
        .bind(user.is_super_admin)
        .bind(&user.role)
        .bind(user.disabled)
        .bind(user.created_at)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres upsert user insert: {e}")))?;
        Ok(user)
    }

    fn delete_user(&self, id: UserId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete user: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("User {id} not found")));
        }
        Ok(())
    }

    fn get_user_by_email(&self, email: &str) -> Result<UserRecord> {
        let email_lower = email.to_lowercase();
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, String, String, Option<String>, bool, String, bool, DateTime<Utc>)>(
                "SELECT id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, COALESCE(disabled, false), created_at FROM users WHERE email = $1",
            )
            .bind(&email_lower)
            .fetch_one(&self.pool),
        )
        .map(|(id, email, name, pw, auth_provider, external_id, is_super_admin, role, disabled, ca)| UserRecord {
            user: User {
                id: UserId(id),
                email,
                name,
                auth_provider,
                external_id,
                is_super_admin,
                role,
                disabled,
                created_at: ca,
            }.normalize_role(),
            password_hash: pw,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("User with email {email} not found")))
    }

    fn get_user(&self, id: UserId) -> Result<User> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, String, Option<String>, bool, String, bool, DateTime<Utc>)>(
                "SELECT id, email, name, auth_provider, external_id, is_super_admin, role, COALESCE(disabled, false), created_at FROM users WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, email, name, auth_provider, external_id, is_super_admin, role, disabled, ca)| User {
            id: UserId(id),
            email,
            name,
            auth_provider,
            external_id,
            is_super_admin,
            role,
            disabled,
            created_at: ca,
        }.normalize_role())
        .map_err(|_| ThaiRagError::NotFound(format!("User {id} not found")))
    }

    fn list_users(&self) -> Vec<User> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, String, Option<String>, bool, String, bool, DateTime<Utc>)>(
                "SELECT id, email, name, auth_provider, external_id, is_super_admin, role, COALESCE(disabled, false), created_at FROM users",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, email, name, auth_provider, external_id, is_super_admin, role, disabled, ca)| User {
            id: UserId(id),
            email,
            name,
            auth_provider,
            external_id,
            is_super_admin,
            role,
            disabled,
            created_at: ca,
        }.normalize_role())
        .collect()
    }

    fn set_user_disabled(&self, id: UserId, disabled: bool) -> Result<User> {
        let result = block_on(
            sqlx::query("UPDATE users SET disabled = $1 WHERE id = $2")
                .bind(disabled)
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres set_user_disabled: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("User {id} not found")));
        }
        self.get_user(id)
    }

    // ── Identity Providers ─────────────────────────────────────────

    fn list_identity_providers(&self) -> Vec<IdentityProvider> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, bool, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, provider_type, enabled, config_json, created_at, updated_at FROM identity_providers",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name, pt, enabled, config_json, ca, ua)| IdentityProvider {
            id: IdpId(id),
            name,
            provider_type: pt,
            enabled,
            config: serde_json::from_str(&config_json).unwrap_or_default(),
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn list_enabled_identity_providers(&self) -> Vec<IdentityProvider> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, bool, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, provider_type, enabled, config_json, created_at, updated_at FROM identity_providers WHERE enabled = TRUE",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name, pt, enabled, config_json, ca, ua)| IdentityProvider {
            id: IdpId(id),
            name,
            provider_type: pt,
            enabled,
            config: serde_json::from_str(&config_json).unwrap_or_default(),
            created_at: ca,
            updated_at: ua,
        })
        .collect()
    }

    fn get_identity_provider(&self, id: IdpId) -> Result<IdentityProvider> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, bool, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, provider_type, enabled, config_json, created_at, updated_at FROM identity_providers WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, name, pt, enabled, config_json, ca, ua)| IdentityProvider {
            id: IdpId(id),
            name,
            provider_type: pt,
            enabled,
            config: serde_json::from_str(&config_json).unwrap_or_default(),
            created_at: ca,
            updated_at: ua,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("Identity provider {id} not found")))
    }

    fn insert_identity_provider(
        &self,
        name: String,
        provider_type: String,
        enabled: bool,
        config: serde_json::Value,
    ) -> Result<IdentityProvider> {
        let now = Utc::now();
        let idp = IdentityProvider {
            id: IdpId::new(),
            name,
            provider_type,
            enabled,
            config: config.clone(),
            created_at: now,
            updated_at: now,
        };
        block_on(sqlx::query(
            "INSERT INTO identity_providers (id, name, provider_type, enabled, config_json, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(idp.id.0)
        .bind(&idp.name)
        .bind(&idp.provider_type)
        .bind(idp.enabled)
        .bind(serde_json::to_string(&config).unwrap_or_default())
        .bind(now)
        .bind(now)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert idp: {e}")))?;
        Ok(idp)
    }

    fn update_identity_provider(
        &self,
        id: IdpId,
        name: String,
        provider_type: String,
        enabled: bool,
        config: serde_json::Value,
    ) -> Result<IdentityProvider> {
        let now = Utc::now();
        let result = block_on(sqlx::query(
            "UPDATE identity_providers SET name = $1, provider_type = $2, enabled = $3, config_json = $4, updated_at = $5 WHERE id = $6",
        )
        .bind(&name)
        .bind(&provider_type)
        .bind(enabled)
        .bind(serde_json::to_string(&config).unwrap_or_default())
        .bind(now)
        .bind(id.0)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update idp: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Identity provider {id} not found"
            )));
        }
        self.get_identity_provider(id)
    }

    fn delete_identity_provider(&self, id: IdpId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM identity_providers WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete idp: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Identity provider {id} not found"
            )));
        }
        Ok(())
    }

    // ── Permissions ─────────────────────────────────────────────────

    fn add_permission(&self, perm: UserPermission) {
        let (level, org_id, dept_id, ws_id) = scope_to_parts(&perm.scope);
        let _ = block_on(sqlx::query(
            "INSERT INTO permissions (user_id, scope_level, org_id, dept_id, workspace_id, role) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(perm.user_id.0)
        .bind(level)
        .bind(org_id)
        .bind(dept_id)
        .bind(ws_id)
        .bind(role_str(&perm.role))
        .execute(&self.pool));
    }

    fn upsert_permission(&self, perm: UserPermission) -> bool {
        let (level, org_id, dept_id, ws_id) = scope_to_parts(&perm.scope);
        let role = role_str(&perm.role);

        let updated = block_on(sqlx::query(
            "UPDATE permissions SET role = $1 WHERE user_id = $2 AND scope_level = $3 AND org_id = $4 AND dept_id = $5 AND workspace_id = $6",
        )
        .bind(role)
        .bind(perm.user_id.0)
        .bind(level)
        .bind(&org_id)
        .bind(&dept_id)
        .bind(&ws_id)
        .execute(&self.pool))
        .map(|r| r.rows_affected())
        .unwrap_or(0);

        if updated > 0 {
            return true;
        }

        let _ = block_on(sqlx::query(
            "INSERT INTO permissions (user_id, scope_level, org_id, dept_id, workspace_id, role) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(perm.user_id.0)
        .bind(level)
        .bind(org_id)
        .bind(dept_id)
        .bind(ws_id)
        .bind(role)
        .execute(&self.pool));
        false
    }

    fn list_permissions_for_org(&self, org_id: OrgId) -> Vec<UserPermission> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, String, String, String)>(
                "SELECT user_id, scope_level, org_id, dept_id, workspace_id, role FROM permissions WHERE org_id = $1",
            )
            .bind(org_id.0.to_string())
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(uid, level, oid, did, wid, role)| UserPermission {
            user_id: UserId(uid),
            scope: parts_to_scope(&level, &oid, &did, &wid),
            role: parse_role(&role),
        })
        .collect()
    }

    fn remove_permission(&self, user_id: UserId, scope: &PermissionScope) -> Result<()> {
        let (level, org_id, dept_id, ws_id) = scope_to_parts(scope);
        let result = block_on(sqlx::query(
            "DELETE FROM permissions WHERE user_id = $1 AND scope_level = $2 AND org_id = $3 AND dept_id = $4 AND workspace_id = $5",
        )
        .bind(user_id.0)
        .bind(level)
        .bind(org_id)
        .bind(dept_id)
        .bind(ws_id)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres remove permission: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound("Permission not found".into()));
        }
        Ok(())
    }

    fn count_org_owners(&self, org_id: OrgId) -> usize {
        block_on(
            sqlx::query_as::<_, (i64,)>(
                "SELECT COUNT(*) FROM permissions WHERE org_id = $1 AND scope_level = 'org' AND role = 'owner'",
            )
            .bind(org_id.0.to_string())
            .fetch_one(&self.pool),
        )
        .map(|(c,)| c as usize)
        .unwrap_or(0)
    }

    fn get_user_role_for_org(&self, user_id: UserId, org_id: OrgId) -> Option<Role> {
        let roles: Vec<Role> = block_on(
            sqlx::query_as::<_, (String,)>(
                "SELECT role FROM permissions WHERE user_id = $1 AND org_id = $2",
            )
            .bind(user_id.0)
            .bind(org_id.0.to_string())
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(r,)| parse_role(&r))
        .collect();
        roles.into_iter().max()
    }

    fn get_user_role_for_dept(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
    ) -> Option<Role> {
        let roles: Vec<Role> = block_on(
            sqlx::query_as::<_, (String,)>(
                "SELECT role FROM permissions WHERE user_id = $1 AND org_id = $2 \
                 AND ((scope_level = 'Org') OR (scope_level = 'Dept' AND dept_id = $3))",
            )
            .bind(user_id.0)
            .bind(org_id.0.to_string())
            .bind(dept_id.0.to_string())
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(r,)| parse_role(&r))
        .collect();
        roles.into_iter().max()
    }

    fn get_user_role_for_workspace(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
        workspace_id: WorkspaceId,
    ) -> Option<Role> {
        let roles: Vec<Role> = block_on(
            sqlx::query_as::<_, (String,)>(
                "SELECT role FROM permissions WHERE user_id = $1 AND org_id = $2 \
                 AND ((scope_level = 'Org') \
                  OR (scope_level = 'Dept' AND dept_id = $3) \
                  OR (scope_level = 'Workspace' AND dept_id = $3 AND workspace_id = $4))",
            )
            .bind(user_id.0)
            .bind(org_id.0.to_string())
            .bind(dept_id.0.to_string())
            .bind(workspace_id.0.to_string())
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(r,)| parse_role(&r))
        .collect();
        roles.into_iter().max()
    }

    fn list_user_permissions(&self, user_id: UserId) -> Vec<UserPermission> {
        block_on(
            sqlx::query_as::<_, (String, String, String, String, String)>(
                "SELECT scope_level, org_id, dept_id, workspace_id, role \
                 FROM permissions WHERE user_id = $1",
            )
            .bind(user_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(level, oid, did, wid, role_str)| UserPermission {
            user_id,
            scope: parts_to_scope(&level, &oid, &did, &wid),
            role: parse_role(&role_str),
        })
        .collect()
    }

    fn get_user_workspace_ids(&self, user_id: UserId) -> Vec<WorkspaceId> {
        let scopes: Vec<PermissionScope> = block_on(
            sqlx::query_as::<_, (String, String, String, String)>(
                "SELECT scope_level, org_id, dept_id, workspace_id FROM permissions WHERE user_id = $1",
            )
            .bind(user_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(level, oid, did, wid)| parts_to_scope(&level, &oid, &did, &wid))
        .collect();

        let mut ws_ids = Vec::new();
        for scope in &scopes {
            match scope {
                PermissionScope::Org { org_id } => {
                    let dept_ids = self.dept_ids_in_org(*org_id);
                    for dept_id in dept_ids {
                        ws_ids.extend(self.workspace_ids_in_dept(dept_id));
                    }
                }
                PermissionScope::Dept { dept_id, .. } => {
                    ws_ids.extend(self.workspace_ids_in_dept(*dept_id));
                }
                PermissionScope::Workspace { workspace_id, .. } => {
                    ws_ids.push(*workspace_id);
                }
            }
        }
        ws_ids.sort();
        ws_ids.dedup();
        ws_ids
    }

    // ── Traversal ───────────────────────────────────────────────────

    fn org_id_for_workspace(&self, workspace_id: WorkspaceId) -> Result<OrgId> {
        let ws = self.get_workspace(workspace_id)?;
        let dept = self.get_dept(ws.dept_id)?;
        Ok(dept.org_id)
    }

    // ── Cascade helpers ─────────────────────────────────────────────

    fn workspace_ids_in_dept(&self, dept_id: DeptId) -> Vec<WorkspaceId> {
        block_on(
            sqlx::query_as::<_, (Uuid,)>("SELECT id FROM workspaces WHERE dept_id = $1")
                .bind(dept_id.0)
                .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id,)| WorkspaceId(id))
        .collect()
    }

    fn dept_ids_in_org(&self, org_id: OrgId) -> Vec<DeptId> {
        block_on(
            sqlx::query_as::<_, (Uuid,)>("SELECT id FROM departments WHERE org_id = $1")
                .bind(org_id.0)
                .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id,)| DeptId(id))
        .collect()
    }

    fn doc_ids_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        block_on(
            sqlx::query_as::<_, (Uuid,)>("SELECT id FROM documents WHERE workspace_id = $1")
                .bind(workspace_id.0)
                .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id,)| DocId(id))
        .collect()
    }

    fn cascade_delete_workspace_docs(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        let doc_ids = self.doc_ids_in_workspace(workspace_id);
        let _ = block_on(
            sqlx::query("DELETE FROM documents WHERE workspace_id = $1")
                .bind(workspace_id.0)
                .execute(&self.pool),
        );
        doc_ids
    }

    fn cascade_delete_workspace(&self, ws_id: WorkspaceId) -> Result<Vec<DocId>> {
        let doc_ids = self.cascade_delete_workspace_docs(ws_id);
        let _ = block_on(
            sqlx::query(
                "DELETE FROM permissions WHERE scope_level = 'workspace' AND workspace_id = $1",
            )
            .bind(ws_id.0.to_string())
            .execute(&self.pool),
        );
        self.delete_workspace(ws_id)?;
        Ok(doc_ids)
    }

    fn cascade_delete_dept(&self, dept_id: DeptId) -> Result<Vec<DocId>> {
        let ws_ids = self.workspace_ids_in_dept(dept_id);
        let mut all_doc_ids = Vec::new();
        for ws_id in &ws_ids {
            all_doc_ids.extend(self.doc_ids_in_workspace(*ws_id));
        }
        // Delete permissions for dept and its workspaces
        let _ = block_on(
            sqlx::query("DELETE FROM permissions WHERE scope_level = 'dept' AND dept_id = $1")
                .bind(dept_id.0.to_string())
                .execute(&self.pool),
        );
        for ws_id in &ws_ids {
            let _ = block_on(
                sqlx::query(
                    "DELETE FROM permissions WHERE scope_level = 'workspace' AND workspace_id = $1",
                )
                .bind(ws_id.0.to_string())
                .execute(&self.pool),
            );
        }
        // CASCADE handles documents and workspaces
        self.delete_dept(dept_id)?;
        Ok(all_doc_ids)
    }

    fn cascade_delete_org(&self, org_id: OrgId) -> Result<Vec<DocId>> {
        let dept_ids = self.dept_ids_in_org(org_id);
        let mut all_doc_ids = Vec::new();
        for dept_id in &dept_ids {
            let ws_ids = self.workspace_ids_in_dept(*dept_id);
            for ws_id in ws_ids {
                all_doc_ids.extend(self.doc_ids_in_workspace(ws_id));
            }
        }
        // Delete all permissions for this org
        let _ = block_on(
            sqlx::query("DELETE FROM permissions WHERE org_id = $1")
                .bind(org_id.0.to_string())
                .execute(&self.pool),
        );
        // CASCADE handles children
        self.delete_org(org_id)?;
        Ok(all_doc_ids)
    }

    fn get_setting(&self, key: &str) -> Option<String> {
        self.get_scoped_setting(key, "global", "")
    }

    fn set_setting(&self, key: &str, value: &str) {
        self.set_scoped_setting(key, "global", "", value);
    }

    fn delete_setting(&self, key: &str) {
        self.delete_scoped_setting(key, "global", "");
    }

    fn list_all_settings(&self) -> Vec<(String, String)> {
        block_on(
            sqlx::query_as::<_, (String, String)>(
                "SELECT key, value FROM settings WHERE scope_type = 'global' AND scope_id = '' \
                 AND key NOT LIKE 'snapshot.%' \
                 AND key NOT LIKE '\\_snapshot\\_index%' \
                 AND key NOT LIKE '\\_embedding\\_fingerprint%'",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
    }

    fn get_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str) -> Option<String> {
        block_on(
            sqlx::query_scalar::<_, String>(
                "SELECT value FROM settings WHERE key = $1 AND scope_type = $2 AND scope_id = $3",
            )
            .bind(key)
            .bind(scope_type)
            .bind(scope_id)
            .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
    }

    fn set_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str, value: &str) {
        let now = chrono::Utc::now();
        let _ = block_on(
            sqlx::query(
                "INSERT INTO settings (key, scope_type, scope_id, value, updated_at) VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (key, scope_type, scope_id) DO UPDATE SET value = $4, updated_at = $5",
            )
            .bind(key)
            .bind(scope_type)
            .bind(scope_id)
            .bind(value)
            .bind(now)
            .execute(&self.pool),
        );
    }

    fn delete_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str) {
        let _ = block_on(
            sqlx::query(
                "DELETE FROM settings WHERE key = $1 AND scope_type = $2 AND scope_id = $3",
            )
            .bind(key)
            .bind(scope_type)
            .bind(scope_id)
            .execute(&self.pool),
        );
    }

    fn list_scoped_settings(&self, scope_type: &str, scope_id: &str) -> Vec<(String, String)> {
        block_on(
            sqlx::query_as::<_, (String, String)>(
                "SELECT key, value FROM settings WHERE scope_type = $1 AND scope_id = $2 \
                 AND key NOT LIKE 'snapshot.%' \
                 AND key NOT LIKE '\\_snapshot\\_index%' \
                 AND key NOT LIKE '\\_embedding\\_fingerprint%'",
            )
            .bind(scope_type)
            .bind(scope_id)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
    }

    fn list_override_keys(&self, scope_type: &str, scope_id: &str) -> Vec<String> {
        block_on(
            sqlx::query_scalar::<_, String>(
                "SELECT key FROM settings WHERE scope_type = $1 AND scope_id = $2",
            )
            .bind(scope_type)
            .bind(scope_id)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
    }

    fn delete_all_scoped_settings(&self, scope_type: &str, scope_id: &str) {
        let _ = block_on(
            sqlx::query("DELETE FROM settings WHERE scope_type = $1 AND scope_id = $2")
                .bind(scope_type)
                .bind(scope_id)
                .execute(&self.pool),
        );
    }

    // ── MCP Connectors ───────────────────────────────────────────────

    fn insert_connector(&self, config: McpConnectorConfig) -> Result<McpConnectorConfig> {
        self.get_workspace(config.workspace_id)?;
        block_on(sqlx::query(
            "INSERT INTO mcp_connectors (id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)",
        )
        .bind(config.id.0)
        .bind(&config.name)
        .bind(&config.description)
        .bind(pg_transport_str(&config.transport))
        .bind(&config.command)
        .bind(serde_json::to_string(&config.args).unwrap_or_default())
        .bind(serde_json::to_string(&config.env).unwrap_or_default())
        .bind(&config.url)
        .bind(serde_json::to_string(&config.headers).unwrap_or_default())
        .bind(config.workspace_id.0)
        .bind(pg_sync_mode_str(&config.sync_mode))
        .bind(&config.schedule_cron)
        .bind(serde_json::to_string(&config.resource_filters).unwrap_or_default())
        .bind(config.max_items_per_sync.map(|v| v as i32))
        .bind(serde_json::to_string(&config.tool_calls).unwrap_or_default())
        .bind(&config.webhook_url)
        .bind(&config.webhook_secret)
        .bind(pg_connector_status_str(&config.status))
        .bind(config.created_at)
        .bind(config.updated_at)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert connector: {e}")))?;
        Ok(config)
    }

    fn get_connector(&self, id: ConnectorId) -> Result<McpConnectorConfig> {
        let row = block_on(
            sqlx::query(
                "SELECT id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at FROM mcp_connectors WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Connector {id} not found")))?;
        Ok(pg_row_to_connector(&row))
    }

    fn list_connectors(&self) -> Vec<McpConnectorConfig> {
        block_on(
            sqlx::query(
                "SELECT id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at FROM mcp_connectors",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .iter()
        .map(pg_row_to_connector)
        .collect()
    }

    fn list_connectors_for_workspace(&self, ws_id: WorkspaceId) -> Vec<McpConnectorConfig> {
        block_on(
            sqlx::query(
                "SELECT id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at FROM mcp_connectors WHERE workspace_id = $1",
            )
            .bind(ws_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .iter()
        .map(pg_row_to_connector)
        .collect()
    }

    fn update_connector(&self, config: McpConnectorConfig) -> Result<()> {
        let result = block_on(sqlx::query(
            "UPDATE mcp_connectors SET name = $1, description = $2, transport = $3, command = $4, args = $5, env = $6, url = $7, headers = $8, workspace_id = $9, sync_mode = $10, schedule_cron = $11, resource_filters = $12, max_items_per_sync = $13, tool_calls = $14, webhook_url = $15, webhook_secret = $16, status = $17, updated_at = $18 WHERE id = $19",
        )
        .bind(&config.name)
        .bind(&config.description)
        .bind(pg_transport_str(&config.transport))
        .bind(&config.command)
        .bind(serde_json::to_string(&config.args).unwrap_or_default())
        .bind(serde_json::to_string(&config.env).unwrap_or_default())
        .bind(&config.url)
        .bind(serde_json::to_string(&config.headers).unwrap_or_default())
        .bind(config.workspace_id.0)
        .bind(pg_sync_mode_str(&config.sync_mode))
        .bind(&config.schedule_cron)
        .bind(serde_json::to_string(&config.resource_filters).unwrap_or_default())
        .bind(config.max_items_per_sync.map(|v| v as i32))
        .bind(serde_json::to_string(&config.tool_calls).unwrap_or_default())
        .bind(&config.webhook_url)
        .bind(&config.webhook_secret)
        .bind(pg_connector_status_str(&config.status))
        .bind(config.updated_at)
        .bind(config.id.0)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update connector: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Connector {} not found",
                config.id
            )));
        }
        Ok(())
    }

    fn delete_connector(&self, id: ConnectorId) -> Result<()> {
        let result = block_on(
            sqlx::query("DELETE FROM mcp_connectors WHERE id = $1")
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete connector: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Connector {id} not found")));
        }
        Ok(())
    }

    fn update_connector_status(&self, id: ConnectorId, status: ConnectorStatus) -> Result<()> {
        let now = Utc::now();
        let result = block_on(
            sqlx::query("UPDATE mcp_connectors SET status = $1, updated_at = $2 WHERE id = $3")
                .bind(pg_connector_status_str(&status))
                .bind(now)
                .bind(id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update connector status: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!("Connector {id} not found")));
        }
        Ok(())
    }

    // ── MCP Sync State ───────────────────────────────────────────────

    fn get_sync_state(&self, connector_id: ConnectorId, resource_uri: &str) -> Option<SyncState> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, Option<Uuid>, DateTime<Utc>, Option<String>)>(
                "SELECT connector_id, resource_uri, content_hash, doc_id, last_synced_at, source_metadata FROM mcp_sync_states WHERE connector_id = $1 AND resource_uri = $2",
            )
            .bind(connector_id.0)
            .bind(resource_uri)
            .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
        .map(|(cid, uri, hash, doc_id, synced, meta)| SyncState {
            connector_id: ConnectorId(cid),
            resource_uri: uri,
            content_hash: hash,
            doc_id: doc_id.map(DocId),
            last_synced_at: synced,
            source_metadata: meta.and_then(|s| serde_json::from_str(&s).ok()),
        })
    }

    fn upsert_sync_state(&self, state: SyncState) -> Result<()> {
        block_on(sqlx::query(
            "INSERT INTO mcp_sync_states (connector_id, resource_uri, content_hash, doc_id, last_synced_at, source_metadata) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (connector_id, resource_uri) DO UPDATE SET content_hash = $3, doc_id = $4, last_synced_at = $5, source_metadata = $6",
        )
        .bind(state.connector_id.0)
        .bind(&state.resource_uri)
        .bind(&state.content_hash)
        .bind(state.doc_id.map(|d| d.0))
        .bind(state.last_synced_at)
        .bind(state.source_metadata.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()))
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres upsert sync state: {e}")))?;
        Ok(())
    }

    fn list_sync_states(&self, connector_id: ConnectorId) -> Vec<SyncState> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, Option<Uuid>, DateTime<Utc>, Option<String>)>(
                "SELECT connector_id, resource_uri, content_hash, doc_id, last_synced_at, source_metadata FROM mcp_sync_states WHERE connector_id = $1",
            )
            .bind(connector_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(cid, uri, hash, doc_id, synced, meta)| SyncState {
            connector_id: ConnectorId(cid),
            resource_uri: uri,
            content_hash: hash,
            doc_id: doc_id.map(DocId),
            last_synced_at: synced,
            source_metadata: meta.and_then(|s| serde_json::from_str(&s).ok()),
        })
        .collect()
    }

    fn delete_sync_states(&self, connector_id: ConnectorId) -> Result<()> {
        block_on(
            sqlx::query("DELETE FROM mcp_sync_states WHERE connector_id = $1")
                .bind(connector_id.0)
                .execute(&self.pool),
        )
        .map_err(|e| ThaiRagError::Internal(format!("Postgres delete sync states: {e}")))?;
        Ok(())
    }

    // ── MCP Sync Runs ────────────────────────────────────────────────

    fn insert_sync_run(&self, run: SyncRun) -> Result<()> {
        block_on(sqlx::query(
            "INSERT INTO mcp_sync_runs (id, connector_id, started_at, completed_at, status, items_discovered, items_created, items_updated, items_skipped, items_failed, error_message) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(run.id.0)
        .bind(run.connector_id.0)
        .bind(run.started_at)
        .bind(run.completed_at)
        .bind(pg_sync_run_status_str(&run.status))
        .bind(run.items_discovered as i32)
        .bind(run.items_created as i32)
        .bind(run.items_updated as i32)
        .bind(run.items_skipped as i32)
        .bind(run.items_failed as i32)
        .bind(&run.error_message)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert sync run: {e}")))?;
        Ok(())
    }

    fn update_sync_run(&self, run: SyncRun) -> Result<()> {
        let result = block_on(sqlx::query(
            "UPDATE mcp_sync_runs SET completed_at = $1, status = $2, items_discovered = $3, items_created = $4, items_updated = $5, items_skipped = $6, items_failed = $7, error_message = $8 WHERE id = $9",
        )
        .bind(run.completed_at)
        .bind(pg_sync_run_status_str(&run.status))
        .bind(run.items_discovered as i32)
        .bind(run.items_created as i32)
        .bind(run.items_updated as i32)
        .bind(run.items_skipped as i32)
        .bind(run.items_failed as i32)
        .bind(&run.error_message)
        .bind(run.id.0)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres update sync run: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Sync run {} not found",
                run.id
            )));
        }
        Ok(())
    }

    fn list_sync_runs(&self, connector_id: ConnectorId, limit: usize) -> Vec<SyncRun> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, DateTime<Utc>, Option<DateTime<Utc>>, String, i32, i32, i32, i32, i32, Option<String>)>(
                "SELECT id, connector_id, started_at, completed_at, status, items_discovered, items_created, items_updated, items_skipped, items_failed, error_message FROM mcp_sync_runs WHERE connector_id = $1 ORDER BY started_at DESC LIMIT $2",
            )
            .bind(connector_id.0)
            .bind(limit as i64)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, cid, started, completed, status, disc, crea, upd, skip, fail, err)| {
            pg_row_to_sync_run(id, cid, started, completed, status, disc, crea, upd, skip, fail, err)
        })
        .collect()
    }

    fn get_latest_sync_run(&self, connector_id: ConnectorId) -> Option<SyncRun> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, DateTime<Utc>, Option<DateTime<Utc>>, String, i32, i32, i32, i32, i32, Option<String>)>(
                "SELECT id, connector_id, started_at, completed_at, status, items_discovered, items_created, items_updated, items_skipped, items_failed, error_message FROM mcp_sync_runs WHERE connector_id = $1 ORDER BY started_at DESC LIMIT 1",
            )
            .bind(connector_id.0)
            .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
        .map(|(id, cid, started, completed, status, disc, crea, upd, skip, fail, err)| {
            pg_row_to_sync_run(id, cid, started, completed, status, disc, crea, upd, skip, fail, err)
        })
    }

    // ── API Key Vault ───────────────────────────────────────────────

    fn list_vault_keys(&self) -> Vec<super::VaultKeyRow> {
        block_on(
            sqlx::query_as::<_, (String, String, String, String, String, String, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at FROM api_key_vault ORDER BY created_at DESC",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at)| {
            super::VaultKeyRow {
                id,
                name,
                provider,
                encrypted_key,
                key_prefix,
                key_suffix,
                base_url,
                created_at: created_at.to_rfc3339(),
                updated_at: updated_at.to_rfc3339(),
            }
        })
        .collect()
    }

    fn get_vault_key(&self, id: &str) -> Option<super::VaultKeyRow> {
        block_on(
            sqlx::query_as::<_, (String, String, String, String, String, String, String, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at FROM api_key_vault WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
        .map(|(id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at)| {
            super::VaultKeyRow {
                id,
                name,
                provider,
                encrypted_key,
                key_prefix,
                key_suffix,
                base_url,
                created_at: created_at.to_rfc3339(),
                updated_at: updated_at.to_rfc3339(),
            }
        })
    }

    fn upsert_vault_key(&self, row: &super::VaultKeyRow) {
        let created_at: DateTime<Utc> = row.created_at.parse().unwrap_or_else(|_| Utc::now());
        let updated_at: DateTime<Utc> = row.updated_at.parse().unwrap_or_else(|_| Utc::now());
        let _ = block_on(
            sqlx::query(
                "INSERT INTO api_key_vault (id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (id) DO UPDATE SET name = $2, provider = $3, encrypted_key = $4, key_prefix = $5, key_suffix = $6, base_url = $7, updated_at = $9",
            )
            .bind(&row.id)
            .bind(&row.name)
            .bind(&row.provider)
            .bind(&row.encrypted_key)
            .bind(&row.key_prefix)
            .bind(&row.key_suffix)
            .bind(&row.base_url)
            .bind(created_at)
            .bind(updated_at)
            .execute(&self.pool),
        );
    }

    fn delete_vault_key(&self, id: &str) {
        let _ = block_on(
            sqlx::query("DELETE FROM api_key_vault WHERE id = $1")
                .bind(id)
                .execute(&self.pool),
        );
    }

    // ── LLM Profiles ────────────────────────────────────────────────

    fn list_llm_profiles(&self) -> Vec<super::LlmProfileRow> {
        block_on(
            sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<i32>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at FROM llm_profiles ORDER BY created_at DESC",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at)| {
            super::LlmProfileRow {
                id,
                name,
                kind,
                model,
                base_url,
                vault_key_id,
                max_tokens: max_tokens.map(|v| v as u32),
                created_at: created_at.to_rfc3339(),
                updated_at: updated_at.to_rfc3339(),
            }
        })
        .collect()
    }

    fn get_llm_profile(&self, id: &str) -> Option<super::LlmProfileRow> {
        block_on(
            sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<i32>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at FROM llm_profiles WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
        .map(|(id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at)| {
            super::LlmProfileRow {
                id,
                name,
                kind,
                model,
                base_url,
                vault_key_id,
                max_tokens: max_tokens.map(|v| v as u32),
                created_at: created_at.to_rfc3339(),
                updated_at: updated_at.to_rfc3339(),
            }
        })
    }

    fn upsert_llm_profile(&self, row: &super::LlmProfileRow) {
        let created_at: DateTime<Utc> = row.created_at.parse().unwrap_or_else(|_| Utc::now());
        let updated_at: DateTime<Utc> = row.updated_at.parse().unwrap_or_else(|_| Utc::now());
        let _ = block_on(
            sqlx::query(
                "INSERT INTO llm_profiles (id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (id) DO UPDATE SET name = $2, kind = $3, model = $4, base_url = $5, vault_key_id = $6, max_tokens = $7, updated_at = $9",
            )
            .bind(&row.id)
            .bind(&row.name)
            .bind(&row.kind)
            .bind(&row.model)
            .bind(&row.base_url)
            .bind(&row.vault_key_id)
            .bind(row.max_tokens.map(|v| v as i32))
            .bind(created_at)
            .bind(updated_at)
            .execute(&self.pool),
        );
    }

    fn delete_llm_profile(&self, id: &str) {
        let _ = block_on(
            sqlx::query("DELETE FROM llm_profiles WHERE id = $1")
                .bind(id)
                .execute(&self.pool),
        );
    }

    // ── Inference Logs ────────────────────────────────────────────────

    fn insert_inference_log(&self, entry: &super::InferenceLogEntry) {
        let _ = block_on(
            sqlx::query(
                "INSERT INTO inference_logs (
                    id, timestamp, user_id, workspace_id, org_id, dept_id, session_id, response_id,
                    query_text, detected_language, intent, complexity,
                    llm_kind, llm_model, settings_scope,
                    prompt_tokens, completion_tokens,
                    total_ms, search_ms, generation_ms,
                    chunks_retrieved, avg_chunk_score, self_rag_decision, self_rag_confidence,
                    quality_guard_pass, relevance_score, hallucination_score, completeness_score,
                    pipeline_route, agents_used,
                    status, error_message, response_length,
                    feedback_score
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8,
                    $9, $10, $11, $12,
                    $13, $14, $15,
                    $16, $17,
                    $18, $19, $20,
                    $21, $22, $23, $24,
                    $25, $26, $27, $28,
                    $29, $30,
                    $31, $32, $33,
                    $34
                )",
            )
            .bind(&entry.id)
            .bind(&entry.timestamp)
            .bind(&entry.user_id)
            .bind(&entry.workspace_id)
            .bind(&entry.org_id)
            .bind(&entry.dept_id)
            .bind(&entry.session_id)
            .bind(&entry.response_id)
            .bind(&entry.query_text)
            .bind(&entry.detected_language)
            .bind(&entry.intent)
            .bind(&entry.complexity)
            .bind(&entry.llm_kind)
            .bind(&entry.llm_model)
            .bind(&entry.settings_scope)
            .bind(entry.prompt_tokens as i32)
            .bind(entry.completion_tokens as i32)
            .bind(entry.total_ms as i64)
            .bind(entry.search_ms.map(|v| v as i64))
            .bind(entry.generation_ms.map(|v| v as i64))
            .bind(entry.chunks_retrieved.map(|v| v as i32))
            .bind(entry.avg_chunk_score)
            .bind(&entry.self_rag_decision)
            .bind(entry.self_rag_confidence)
            .bind(entry.quality_guard_pass)
            .bind(entry.relevance_score)
            .bind(entry.hallucination_score)
            .bind(entry.completeness_score)
            .bind(&entry.pipeline_route)
            .bind(&entry.agents_used)
            .bind(&entry.status)
            .bind(&entry.error_message)
            .bind(entry.response_length as i32)
            .bind(entry.feedback_score.map(|v| v as i16))
            .execute(&self.pool),
        );

        // Log retention: if count exceeds 50000, delete oldest 10%
        let count: i64 = block_on(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM inference_logs")
                .fetch_one(&self.pool),
        )
        .unwrap_or(0);
        if count > 50_000 {
            let to_delete = count / 10;
            let _ = block_on(
                sqlx::query(
                    "DELETE FROM inference_logs WHERE id IN (
                        SELECT id FROM inference_logs ORDER BY timestamp ASC LIMIT $1
                    )",
                )
                .bind(to_delete)
                .execute(&self.pool),
            );
        }
    }

    fn list_inference_logs(
        &self,
        filter: &super::InferenceLogFilter,
    ) -> Vec<super::InferenceLogEntry> {
        let mut sql = String::from(
            "SELECT id, timestamp, user_id, workspace_id, org_id, dept_id, session_id, response_id,
                    query_text, detected_language, intent, complexity,
                    llm_kind, llm_model, settings_scope,
                    prompt_tokens, completion_tokens,
                    total_ms, search_ms, generation_ms,
                    chunks_retrieved, avg_chunk_score, self_rag_decision, self_rag_confidence,
                    quality_guard_pass, relevance_score, hallucination_score, completeness_score,
                    pipeline_route, agents_used,
                    status, error_message, response_length,
                    feedback_score
             FROM inference_logs WHERE 1=1",
        );
        let mut param_idx: usize = 1;
        let mut params_ws = None;
        let mut params_user = None;
        let mut params_from = None;
        let mut params_to = None;
        let mut params_status = None;
        let mut params_model = None;
        let mut params_intent = None;
        let mut params_response_id = None;
        let mut params_session_id = None;

        if let Some(ref ws) = filter.workspace_id {
            sql.push_str(&format!(" AND workspace_id = ${param_idx}"));
            params_ws = Some(ws.clone());
            param_idx += 1;
        }
        if let Some(ref u) = filter.user_id {
            sql.push_str(&format!(" AND user_id = ${param_idx}"));
            params_user = Some(u.clone());
            param_idx += 1;
        }
        if let Some(ref from) = filter.from_timestamp {
            sql.push_str(&format!(" AND timestamp >= ${param_idx}"));
            params_from = Some(from.clone());
            param_idx += 1;
        }
        if let Some(ref to) = filter.to_timestamp {
            sql.push_str(&format!(" AND timestamp <= ${param_idx}"));
            params_to = Some(to.clone());
            param_idx += 1;
        }
        if let Some(ref st) = filter.status {
            sql.push_str(&format!(" AND status = ${param_idx}"));
            params_status = Some(st.clone());
            param_idx += 1;
        }
        if let Some(ref model) = filter.llm_model {
            sql.push_str(&format!(" AND llm_model = ${param_idx}"));
            params_model = Some(model.clone());
            param_idx += 1;
        }
        if let Some(ref intent) = filter.intent {
            sql.push_str(&format!(" AND intent = ${param_idx}"));
            params_intent = Some(intent.clone());
            param_idx += 1;
        }
        if let Some(ref rid) = filter.response_id {
            sql.push_str(&format!(" AND response_id = ${param_idx}"));
            params_response_id = Some(rid.clone());
            param_idx += 1;
        }
        if let Some(ref sid) = filter.session_id {
            sql.push_str(&format!(" AND session_id = ${param_idx}"));
            params_session_id = Some(sid.clone());
            param_idx += 1;
        }

        sql.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT ${} OFFSET ${}",
            param_idx,
            param_idx + 1
        ));

        block_on(async {
            let mut q = sqlx::query(&sql);
            if let Some(ref v) = params_ws {
                q = q.bind(v);
            }
            if let Some(ref v) = params_user {
                q = q.bind(v);
            }
            if let Some(ref v) = params_from {
                q = q.bind(v);
            }
            if let Some(ref v) = params_to {
                q = q.bind(v);
            }
            if let Some(ref v) = params_status {
                q = q.bind(v);
            }
            if let Some(ref v) = params_model {
                q = q.bind(v);
            }
            if let Some(ref v) = params_intent {
                q = q.bind(v);
            }
            if let Some(ref v) = params_response_id {
                q = q.bind(v);
            }
            if let Some(ref v) = params_session_id {
                q = q.bind(v);
            }
            q = q.bind(filter.limit as i64);
            q = q.bind(filter.offset as i64);

            q.fetch_all(&self.pool).await
        })
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            let feedback_raw: Option<i16> = row.get("feedback_score");
            super::InferenceLogEntry {
                id: row.get("id"),
                timestamp: row.get("timestamp"),
                user_id: row.get("user_id"),
                workspace_id: row.get("workspace_id"),
                org_id: row.get("org_id"),
                dept_id: row.get("dept_id"),
                session_id: row.get("session_id"),
                response_id: row.get("response_id"),
                query_text: row.get("query_text"),
                detected_language: row.get("detected_language"),
                intent: row.get("intent"),
                complexity: row.get("complexity"),
                llm_kind: row.get("llm_kind"),
                llm_model: row.get("llm_model"),
                settings_scope: row.get("settings_scope"),
                prompt_tokens: row.get::<i32, _>("prompt_tokens") as u32,
                completion_tokens: row.get::<i32, _>("completion_tokens") as u32,
                total_ms: row.get::<i64, _>("total_ms") as u64,
                search_ms: row.get::<Option<i64>, _>("search_ms").map(|v| v as u64),
                generation_ms: row.get::<Option<i64>, _>("generation_ms").map(|v| v as u64),
                chunks_retrieved: row
                    .get::<Option<i32>, _>("chunks_retrieved")
                    .map(|v| v as u32),
                avg_chunk_score: row.get("avg_chunk_score"),
                self_rag_decision: row.get("self_rag_decision"),
                self_rag_confidence: row.get("self_rag_confidence"),
                quality_guard_pass: row.get("quality_guard_pass"),
                relevance_score: row.get("relevance_score"),
                hallucination_score: row.get("hallucination_score"),
                completeness_score: row.get("completeness_score"),
                pipeline_route: row.get("pipeline_route"),
                agents_used: row.get("agents_used"),
                status: row.get("status"),
                error_message: row.get("error_message"),
                response_length: row.get::<i32, _>("response_length") as u32,
                feedback_score: feedback_raw.map(|v| v as i8),
            }
        })
        .collect()
    }

    fn get_inference_stats(&self, filter: &super::InferenceLogFilter) -> super::InferenceStats {
        // Build the WHERE clause shared by all sub-queries
        let mut where_clause = String::from("WHERE 1=1");
        let mut param_idx: usize = 1;
        let mut params: Vec<String> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            where_clause.push_str(&format!(" AND workspace_id = ${param_idx}"));
            params.push(ws.clone());
            param_idx += 1;
        }
        if let Some(ref u) = filter.user_id {
            where_clause.push_str(&format!(" AND user_id = ${param_idx}"));
            params.push(u.clone());
            param_idx += 1;
        }
        if let Some(ref from) = filter.from_timestamp {
            where_clause.push_str(&format!(" AND timestamp >= ${param_idx}"));
            params.push(from.clone());
            param_idx += 1;
        }
        if let Some(ref to) = filter.to_timestamp {
            where_clause.push_str(&format!(" AND timestamp <= ${param_idx}"));
            params.push(to.clone());
            param_idx += 1;
        }
        if let Some(ref st) = filter.status {
            where_clause.push_str(&format!(" AND status = ${param_idx}"));
            params.push(st.clone());
            param_idx += 1;
        }
        if let Some(ref model) = filter.llm_model {
            where_clause.push_str(&format!(" AND llm_model = ${param_idx}"));
            params.push(model.clone());
            param_idx += 1;
        }
        if let Some(ref intent) = filter.intent {
            where_clause.push_str(&format!(" AND intent = ${param_idx}"));
            params.push(intent.clone());
            param_idx += 1;
        }
        if let Some(ref rid) = filter.response_id {
            where_clause.push_str(&format!(" AND response_id = ${param_idx}"));
            params.push(rid.clone());
            param_idx += 1;
        }
        if let Some(ref sid) = filter.session_id {
            where_clause.push_str(&format!(" AND session_id = ${param_idx}"));
            params.push(sid.clone());
            param_idx += 1;
        }
        let _ = param_idx; // suppress unused warning

        // Aggregate query
        let agg_sql = format!(
            "SELECT
                COUNT(*)::BIGINT AS total_requests,
                COALESCE(AVG(total_ms), 0)::FLOAT8 AS avg_total_ms,
                COALESCE(AVG(search_ms), 0)::FLOAT8 AS avg_search_ms,
                COALESCE(AVG(generation_ms), 0)::FLOAT8 AS avg_generation_ms,
                COALESCE(AVG(relevance_score), 0)::FLOAT8 AS avg_relevance_score,
                COALESCE(SUM(prompt_tokens), 0)::BIGINT AS total_prompt_tokens,
                COALESCE(SUM(completion_tokens), 0)::BIGINT AS total_completion_tokens,
                CASE WHEN COUNT(*) > 0
                    THEN COUNT(*) FILTER (WHERE status = 'success')::FLOAT8 / COUNT(*)::FLOAT8
                    ELSE 0 END AS success_rate,
                CASE WHEN COUNT(*) FILTER (WHERE quality_guard_pass IS NOT NULL) > 0
                    THEN COUNT(*) FILTER (WHERE quality_guard_pass = TRUE)::FLOAT8
                         / COUNT(*) FILTER (WHERE quality_guard_pass IS NOT NULL)::FLOAT8
                    ELSE 0 END AS quality_pass_rate,
                CASE WHEN COUNT(*) FILTER (WHERE feedback_score IS NOT NULL) > 0
                    THEN COUNT(*) FILTER (WHERE feedback_score > 0)::FLOAT8
                         / COUNT(*) FILTER (WHERE feedback_score IS NOT NULL)::FLOAT8
                    ELSE 0 END AS feedback_positive_rate
             FROM inference_logs {where_clause}"
        );

        let agg_row = block_on(async {
            let mut q = sqlx::query(&agg_sql);
            for p in &params {
                q = q.bind(p);
            }
            q.fetch_one(&self.pool).await
        });

        let (
            total_requests,
            avg_total_ms,
            avg_search_ms,
            avg_generation_ms,
            avg_relevance_score,
            total_prompt_tokens,
            total_completion_tokens,
            success_rate,
            quality_pass_rate,
            feedback_positive_rate,
        ) = match agg_row {
            Ok(row) => (
                row.get::<i64, _>("total_requests") as u64,
                row.get::<f64, _>("avg_total_ms"),
                row.get::<f64, _>("avg_search_ms"),
                row.get::<f64, _>("avg_generation_ms"),
                row.get::<f64, _>("avg_relevance_score"),
                row.get::<i64, _>("total_prompt_tokens") as u64,
                row.get::<i64, _>("total_completion_tokens") as u64,
                row.get::<f64, _>("success_rate"),
                row.get::<f64, _>("quality_pass_rate"),
                row.get::<f64, _>("feedback_positive_rate"),
            ),
            Err(_) => (0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0.0, 0.0, 0.0),
        };

        // By model
        let model_sql = format!(
            "SELECT llm_model,
                    COUNT(*)::BIGINT AS count,
                    COALESCE(AVG(total_ms), 0)::FLOAT8 AS avg_ms,
                    COALESCE(AVG(relevance_score), 0)::FLOAT8 AS avg_quality,
                    COALESCE(SUM(prompt_tokens) + SUM(completion_tokens), 0)::BIGINT AS total_tokens
             FROM inference_logs {where_clause}
             GROUP BY llm_model ORDER BY count DESC"
        );

        let by_model = block_on(async {
            let mut q = sqlx::query(&model_sql);
            for p in &params {
                q = q.bind(p);
            }
            q.fetch_all(&self.pool).await
        })
        .unwrap_or_default()
        .into_iter()
        .map(|row| super::ModelStats {
            model: row.get("llm_model"),
            count: row.get::<i64, _>("count") as u64,
            avg_ms: row.get::<f64, _>("avg_ms"),
            avg_quality: row.get::<f64, _>("avg_quality"),
            total_tokens: row.get::<i64, _>("total_tokens") as u64,
        })
        .collect();

        // By workspace
        let ws_sql = format!(
            "SELECT COALESCE(workspace_id, '') AS workspace_id,
                    COUNT(*)::BIGINT AS count,
                    COALESCE(AVG(total_ms), 0)::FLOAT8 AS avg_ms,
                    COALESCE(SUM(prompt_tokens) + SUM(completion_tokens), 0)::BIGINT AS total_tokens
             FROM inference_logs {where_clause}
             GROUP BY workspace_id ORDER BY count DESC"
        );

        let by_workspace = block_on(async {
            let mut q = sqlx::query(&ws_sql);
            for p in &params {
                q = q.bind(p);
            }
            q.fetch_all(&self.pool).await
        })
        .unwrap_or_default()
        .into_iter()
        .map(|row| super::WorkspaceStats {
            workspace_id: row.get("workspace_id"),
            count: row.get::<i64, _>("count") as u64,
            avg_ms: row.get::<f64, _>("avg_ms"),
            total_tokens: row.get::<i64, _>("total_tokens") as u64,
        })
        .collect();

        super::InferenceStats {
            total_requests,
            avg_total_ms,
            avg_search_ms,
            avg_generation_ms,
            avg_relevance_score,
            total_prompt_tokens,
            total_completion_tokens,
            success_rate,
            quality_pass_rate,
            feedback_positive_rate,
            by_model,
            by_workspace,
        }
    }

    fn update_inference_log_feedback(&self, response_id: &str, score: i8) {
        let _ = block_on(
            sqlx::query("UPDATE inference_logs SET feedback_score = $1 WHERE response_id = $2")
                .bind(score as i16)
                .bind(response_id)
                .execute(&self.pool),
        );
    }

    fn delete_inference_logs(&self, filter: &super::InferenceLogFilter) -> u64 {
        let mut sql = String::from("DELETE FROM inference_logs WHERE 1=1");
        let mut param_idx: usize = 1;
        let mut params: Vec<String> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            sql.push_str(&format!(" AND workspace_id = ${param_idx}"));
            params.push(ws.clone());
            param_idx += 1;
        }
        if let Some(ref u) = filter.user_id {
            sql.push_str(&format!(" AND user_id = ${param_idx}"));
            params.push(u.clone());
            param_idx += 1;
        }
        if let Some(ref from) = filter.from_timestamp {
            sql.push_str(&format!(" AND timestamp >= ${param_idx}"));
            params.push(from.clone());
            param_idx += 1;
        }
        if let Some(ref to) = filter.to_timestamp {
            sql.push_str(&format!(" AND timestamp <= ${param_idx}"));
            params.push(to.clone());
            param_idx += 1;
        }
        if let Some(ref st) = filter.status {
            sql.push_str(&format!(" AND status = ${param_idx}"));
            params.push(st.clone());
            param_idx += 1;
        }
        if let Some(ref model) = filter.llm_model {
            sql.push_str(&format!(" AND llm_model = ${param_idx}"));
            params.push(model.clone());
            param_idx += 1;
        }
        if let Some(ref intent) = filter.intent {
            sql.push_str(&format!(" AND intent = ${param_idx}"));
            params.push(intent.clone());
            param_idx += 1;
        }
        if let Some(ref rid) = filter.response_id {
            sql.push_str(&format!(" AND response_id = ${param_idx}"));
            params.push(rid.clone());
            param_idx += 1;
        }
        if let Some(ref sid) = filter.session_id {
            sql.push_str(&format!(" AND session_id = ${param_idx}"));
            params.push(sid.clone());
            param_idx += 1;
        }
        let _ = param_idx;

        block_on(async {
            let mut q = sqlx::query(&sql);
            for p in &params {
                q = q.bind(p);
            }
            q.execute(&self.pool).await
        })
        .map(|r| r.rows_affected())
        .unwrap_or(0)
    }

    fn count_inference_logs(&self, filter: &super::InferenceLogFilter) -> u64 {
        let mut sql = String::from("SELECT COUNT(*)::BIGINT FROM inference_logs WHERE 1=1");
        let mut param_idx: usize = 1;
        let mut params: Vec<String> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            sql.push_str(&format!(" AND workspace_id = ${param_idx}"));
            params.push(ws.clone());
            param_idx += 1;
        }
        if let Some(ref u) = filter.user_id {
            sql.push_str(&format!(" AND user_id = ${param_idx}"));
            params.push(u.clone());
            param_idx += 1;
        }
        if let Some(ref from) = filter.from_timestamp {
            sql.push_str(&format!(" AND timestamp >= ${param_idx}"));
            params.push(from.clone());
            param_idx += 1;
        }
        if let Some(ref to) = filter.to_timestamp {
            sql.push_str(&format!(" AND timestamp <= ${param_idx}"));
            params.push(to.clone());
            param_idx += 1;
        }
        if let Some(ref st) = filter.status {
            sql.push_str(&format!(" AND status = ${param_idx}"));
            params.push(st.clone());
            param_idx += 1;
        }
        if let Some(ref model) = filter.llm_model {
            sql.push_str(&format!(" AND llm_model = ${param_idx}"));
            params.push(model.clone());
            param_idx += 1;
        }
        if let Some(ref intent) = filter.intent {
            sql.push_str(&format!(" AND intent = ${param_idx}"));
            params.push(intent.clone());
            param_idx += 1;
        }
        if let Some(ref rid) = filter.response_id {
            sql.push_str(&format!(" AND response_id = ${param_idx}"));
            params.push(rid.clone());
            param_idx += 1;
        }
        if let Some(ref sid) = filter.session_id {
            sql.push_str(&format!(" AND session_id = ${param_idx}"));
            params.push(sid.clone());
            param_idx += 1;
        }
        let _ = param_idx;

        block_on(async {
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            for p in &params {
                q = q.bind(p);
            }
            q.fetch_one(&self.pool).await
        })
        .unwrap_or(0) as u64
    }

    // ── API Keys (M2M Auth) ──────────────────────────────────────────

    fn create_api_key(
        &self,
        user_id: UserId,
        name: String,
        key_hash: String,
        key_prefix: String,
        role: String,
    ) -> Result<super::ApiKeyRow> {
        let id = ApiKeyId::new();
        let now = chrono::Utc::now();
        block_on(async {
            sqlx::query(
                "INSERT INTO api_keys (id, name, key_hash, key_prefix, user_id, role, created_at, is_active)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)",
            )
            .bind(id.0)
            .bind(&name)
            .bind(&key_hash)
            .bind(&key_prefix)
            .bind(user_id.0)
            .bind(&role)
            .bind(now)
            .execute(&self.pool)
            .await
        })
        .map_err(|e| ThaiRagError::Database(format!("Failed to create API key: {e}")))?;

        Ok(super::ApiKeyRow {
            id,
            name,
            key_hash,
            key_prefix,
            user_id,
            role,
            created_at: now.to_rfc3339(),
            last_used_at: None,
            is_active: true,
        })
    }

    fn get_api_key_by_hash(&self, key_hash: &str) -> Option<super::ApiKeyRow> {
        block_on(async {
            sqlx::query(
                "SELECT id, name, key_hash, key_prefix, user_id, role, created_at, last_used_at, is_active
                 FROM api_keys WHERE key_hash = $1",
            )
            .bind(key_hash)
            .fetch_optional(&self.pool)
            .await
        })
        .ok()
        .flatten()
        .map(|row| {
            let id: Uuid = row.get("id");
            let user_id: Uuid = row.get("user_id");
            let created_at: DateTime<Utc> = row.get("created_at");
            let last_used_at: Option<DateTime<Utc>> = row.get("last_used_at");
            super::ApiKeyRow {
                id: ApiKeyId(id),
                name: row.get("name"),
                key_hash: row.get("key_hash"),
                key_prefix: row.get("key_prefix"),
                user_id: UserId(user_id),
                role: row.get("role"),
                created_at: created_at.to_rfc3339(),
                last_used_at: last_used_at.map(|dt| dt.to_rfc3339()),
                is_active: row.get("is_active"),
            }
        })
    }

    fn list_api_keys(&self, user_id: UserId) -> Vec<super::ApiKeyRow> {
        block_on(async {
            sqlx::query(
                "SELECT id, name, key_hash, key_prefix, user_id, role, created_at, last_used_at, is_active
                 FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC",
            )
            .bind(user_id.0)
            .fetch_all(&self.pool)
            .await
        })
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            let id: Uuid = row.get("id");
            let uid: Uuid = row.get("user_id");
            let created_at: DateTime<Utc> = row.get("created_at");
            let last_used_at: Option<DateTime<Utc>> = row.get("last_used_at");
            super::ApiKeyRow {
                id: ApiKeyId(id),
                name: row.get("name"),
                key_hash: row.get("key_hash"),
                key_prefix: row.get("key_prefix"),
                user_id: UserId(uid),
                role: row.get("role"),
                created_at: created_at.to_rfc3339(),
                last_used_at: last_used_at.map(|dt| dt.to_rfc3339()),
                is_active: row.get("is_active"),
            }
        })
        .collect()
    }

    fn revoke_api_key(&self, key_id: ApiKeyId) -> Result<()> {
        let result = block_on(async {
            sqlx::query("UPDATE api_keys SET is_active = FALSE WHERE id = $1")
                .bind(key_id.0)
                .execute(&self.pool)
                .await
        })
        .map_err(|e| ThaiRagError::Database(format!("Failed to revoke API key: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "API key {key_id} not found"
            )));
        }
        Ok(())
    }

    fn touch_api_key(&self, key_id: ApiKeyId) {
        let now = chrono::Utc::now();
        let _ = block_on(async {
            sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE id = $2")
                .bind(now)
                .bind(key_id.0)
                .execute(&self.pool)
                .await
        });
    }
}
