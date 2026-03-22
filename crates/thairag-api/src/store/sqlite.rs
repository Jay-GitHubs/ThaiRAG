use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, DocStatus, Document, IdentityProvider, Organization, PermissionScope, User,
    UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{
    ConnectorId, ConnectorStatus, DeptId, DocId, IdpId, McpConnectorConfig, McpTransport, OrgId,
    SyncMode, SyncRun, SyncRunId, SyncRunStatus, SyncState, UserId, WorkspaceId,
};
use uuid::Uuid;

use super::{KmStoreTrait, UserRecord};

type Result<T> = std::result::Result<T, ThaiRagError>;

pub struct SqliteKmStore {
    conn: Mutex<Connection>,
}

impl SqliteKmStore {
    pub fn new(db_url: &str) -> std::result::Result<Self, ThaiRagError> {
        let conn = Connection::open(db_url)
            .map_err(|e| ThaiRagError::Config(format!("SQLite open failed: {e}")))?;

        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(|e| ThaiRagError::Config(format!("SQLite pragma failed: {e}")))?;

        let schema = include_str!("schema.sql");
        conn.execute_batch(schema)
            .map_err(|e| ThaiRagError::Config(format!("SQLite schema failed: {e}")))?;

        // Migrations for existing databases — add columns if missing
        for stmt in &[
            "ALTER TABLE users ADD COLUMN auth_provider TEXT NOT NULL DEFAULT 'local'",
            "ALTER TABLE users ADD COLUMN external_id TEXT",
            "ALTER TABLE users ADD COLUMN is_super_admin INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'viewer'",
            "ALTER TABLE documents ADD COLUMN processing_step TEXT",
        ] {
            let _ = conn.execute_batch(stmt); // ignore "duplicate column" errors
        }

        // Fix existing super admins that have default 'viewer' role
        let _ = conn.execute_batch(
            "UPDATE users SET role = 'super_admin' WHERE is_super_admin = 1 AND role = 'viewer'",
        );

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

// ── Helper functions ────────────────────────────────────────────────

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn ts(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn parse_uuid(s: &str) -> Uuid {
    s.parse().unwrap_or_default()
}

fn doc_from_row(row: &rusqlite::Row) -> rusqlite::Result<Document> {
    let id_s: String = row.get(0)?;
    let ws_s: String = row.get(1)?;
    let title: String = row.get(2)?;
    let mime: String = row.get(3)?;
    let size: i64 = row.get(4)?;
    let status_s: String = row.get(5)?;
    let chunk_count: i64 = row.get(6)?;
    let error_message: Option<String> = row.get(7)?;
    let processing_step: Option<String> = row.get(8)?;
    let ca: String = row.get(9)?;
    let ua: String = row.get(10)?;
    Ok(Document {
        id: DocId(parse_uuid(&id_s)),
        workspace_id: WorkspaceId(parse_uuid(&ws_s)),
        title,
        mime_type: mime,
        size_bytes: size,
        status: DocStatus::from_str_lossy(&status_s),
        chunk_count,
        error_message,
        processing_step,
        created_at: parse_ts(&ca),
        updated_at: parse_ts(&ua),
    })
}

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
            org_id: OrgId(parse_uuid(org_id)),
            dept_id: DeptId(parse_uuid(dept_id)),
        },
        "workspace" => PermissionScope::Workspace {
            org_id: OrgId(parse_uuid(org_id)),
            dept_id: DeptId(parse_uuid(dept_id)),
            workspace_id: WorkspaceId(parse_uuid(ws_id)),
        },
        _ => PermissionScope::Org {
            org_id: OrgId(parse_uuid(org_id)),
        },
    }
}

fn parse_transport(s: &str) -> McpTransport {
    match s {
        "sse" => McpTransport::Sse,
        _ => McpTransport::Stdio,
    }
}

fn parse_connector_status(s: &str) -> ConnectorStatus {
    match s {
        "paused" => ConnectorStatus::Paused,
        "error" => ConnectorStatus::Error,
        "syncing" => ConnectorStatus::Syncing,
        _ => ConnectorStatus::Active,
    }
}

fn parse_sync_mode(s: &str) -> SyncMode {
    match s {
        "scheduled" => SyncMode::Scheduled,
        _ => SyncMode::OnDemand,
    }
}

fn parse_sync_run_status(s: &str) -> SyncRunStatus {
    match s {
        "completed" => SyncRunStatus::Completed,
        "failed" => SyncRunStatus::Failed,
        "cancelled" => SyncRunStatus::Cancelled,
        _ => SyncRunStatus::Running,
    }
}

fn connector_from_row(row: &rusqlite::Row) -> rusqlite::Result<McpConnectorConfig> {
    let id_s: String = row.get(0)?;
    let name: String = row.get(1)?;
    let description: String = row.get(2)?;
    let transport_s: String = row.get(3)?;
    let command: Option<String> = row.get(4)?;
    let args_s: String = row.get(5)?;
    let env_s: String = row.get(6)?;
    let url: Option<String> = row.get(7)?;
    let headers_s: String = row.get(8)?;
    let ws_s: String = row.get(9)?;
    let sync_mode_s: String = row.get(10)?;
    let schedule_cron: Option<String> = row.get(11)?;
    let resource_filters_s: String = row.get(12)?;
    let max_items: Option<i64> = row.get(13)?;
    let tool_calls_s: String = row.get(14)?;
    let webhook_url: Option<String> = row.get(15)?;
    let webhook_secret: Option<String> = row.get(16)?;
    let status_s: String = row.get(17)?;
    let ca: String = row.get(18)?;
    let ua: String = row.get(19)?;
    Ok(McpConnectorConfig {
        id: ConnectorId(parse_uuid(&id_s)),
        name,
        description,
        transport: parse_transport(&transport_s),
        command,
        args: serde_json::from_str(&args_s).unwrap_or_default(),
        env: serde_json::from_str(&env_s).unwrap_or_default(),
        url,
        headers: serde_json::from_str(&headers_s).unwrap_or_default(),
        workspace_id: WorkspaceId(parse_uuid(&ws_s)),
        sync_mode: parse_sync_mode(&sync_mode_s),
        schedule_cron,
        resource_filters: serde_json::from_str(&resource_filters_s).unwrap_or_default(),
        max_items_per_sync: max_items.map(|v| v as usize),
        tool_calls: serde_json::from_str(&tool_calls_s).unwrap_or_default(),
        webhook_url,
        webhook_secret,
        status: parse_connector_status(&status_s),
        created_at: parse_ts(&ca),
        updated_at: parse_ts(&ua),
    })
}

fn sync_state_from_row(row: &rusqlite::Row) -> rusqlite::Result<SyncState> {
    let cid_s: String = row.get(0)?;
    let resource_uri: String = row.get(1)?;
    let content_hash: String = row.get(2)?;
    let doc_id_s: Option<String> = row.get(3)?;
    let last_synced: String = row.get(4)?;
    let meta_s: Option<String> = row.get(5)?;
    Ok(SyncState {
        connector_id: ConnectorId(parse_uuid(&cid_s)),
        resource_uri,
        content_hash,
        doc_id: doc_id_s.map(|s| DocId(parse_uuid(&s))),
        last_synced_at: parse_ts(&last_synced),
        source_metadata: meta_s.and_then(|s| serde_json::from_str(&s).ok()),
    })
}

fn sync_run_from_row(row: &rusqlite::Row) -> rusqlite::Result<SyncRun> {
    let id_s: String = row.get(0)?;
    let cid_s: String = row.get(1)?;
    let started: String = row.get(2)?;
    let completed: Option<String> = row.get(3)?;
    let status_s: String = row.get(4)?;
    let discovered: i64 = row.get(5)?;
    let created: i64 = row.get(6)?;
    let updated: i64 = row.get(7)?;
    let skipped: i64 = row.get(8)?;
    let failed: i64 = row.get(9)?;
    let error_message: Option<String> = row.get(10)?;
    Ok(SyncRun {
        id: SyncRunId(parse_uuid(&id_s)),
        connector_id: ConnectorId(parse_uuid(&cid_s)),
        started_at: parse_ts(&started),
        completed_at: completed.map(|s| parse_ts(&s)),
        status: parse_sync_run_status(&status_s),
        items_discovered: discovered as usize,
        items_created: created as usize,
        items_updated: updated as usize,
        items_skipped: skipped as usize,
        items_failed: failed as usize,
        error_message,
    })
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

// ── KmStoreTrait implementation ─────────────────────────────────────

impl KmStoreTrait for SqliteKmStore {
    // ── Organization ────────────────────────────────────────────────

    fn insert_org(&self, name: String) -> Result<Organization> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let org = Organization {
            id: OrgId::new(),
            name,
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO organizations (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![org.id.0.to_string(), org.name, ts(&now), ts(&now)],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert org: {e}")))?;
        Ok(org)
    }

    fn get_org(&self, id: OrgId) -> Result<Organization> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, created_at, updated_at FROM organizations WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let name: String = row.get(1)?;
                let ca: String = row.get(2)?;
                let ua: String = row.get(3)?;
                Ok(Organization {
                    id: OrgId(parse_uuid(&id_s)),
                    name,
                    created_at: parse_ts(&ca),
                    updated_at: parse_ts(&ua),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Organization {id} not found")))
    }

    fn list_orgs(&self) -> Vec<Organization> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, created_at, updated_at FROM organizations")
            .unwrap();
        stmt.query_map([], |row| {
            let id_s: String = row.get(0)?;
            let name: String = row.get(1)?;
            let ca: String = row.get(2)?;
            let ua: String = row.get(3)?;
            Ok(Organization {
                id: OrgId(parse_uuid(&id_s)),
                name,
                created_at: parse_ts(&ca),
                updated_at: parse_ts(&ua),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_org(&self, id: OrgId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM organizations WHERE id = ?1",
                params![id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete org: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Organization {id} not found"
            )));
        }
        Ok(())
    }

    // ── Department ──────────────────────────────────────────────────

    fn insert_dept(&self, org_id: OrgId, name: String) -> Result<Department> {
        self.get_org(org_id)?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let dept = Department {
            id: DeptId::new(),
            org_id,
            name,
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO departments (id, org_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![dept.id.0.to_string(), org_id.0.to_string(), dept.name, ts(&now), ts(&now)],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert dept: {e}")))?;
        Ok(dept)
    }

    fn get_dept(&self, id: DeptId) -> Result<Department> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, org_id, name, created_at, updated_at FROM departments WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let org_s: String = row.get(1)?;
                let name: String = row.get(2)?;
                let ca: String = row.get(3)?;
                let ua: String = row.get(4)?;
                Ok(Department {
                    id: DeptId(parse_uuid(&id_s)),
                    org_id: OrgId(parse_uuid(&org_s)),
                    name,
                    created_at: parse_ts(&ca),
                    updated_at: parse_ts(&ua),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Department {id} not found")))
    }

    fn list_depts_in_org(&self, org_id: OrgId) -> Vec<Department> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, org_id, name, created_at, updated_at FROM departments WHERE org_id = ?1")
            .unwrap();
        stmt.query_map(params![org_id.0.to_string()], |row| {
            let id_s: String = row.get(0)?;
            let org_s: String = row.get(1)?;
            let name: String = row.get(2)?;
            let ca: String = row.get(3)?;
            let ua: String = row.get(4)?;
            Ok(Department {
                id: DeptId(parse_uuid(&id_s)),
                org_id: OrgId(parse_uuid(&org_s)),
                name,
                created_at: parse_ts(&ca),
                updated_at: parse_ts(&ua),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_dept(&self, id: DeptId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM departments WHERE id = ?1",
                params![id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete dept: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Department {id} not found")));
        }
        Ok(())
    }

    // ── Workspace ───────────────────────────────────────────────────

    fn insert_workspace(&self, dept_id: DeptId, name: String) -> Result<Workspace> {
        self.get_dept(dept_id)?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let ws = Workspace {
            id: WorkspaceId::new(),
            dept_id,
            name,
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO workspaces (id, dept_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ws.id.0.to_string(), dept_id.0.to_string(), ws.name, ts(&now), ts(&now)],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert workspace: {e}")))?;
        Ok(ws)
    }

    fn get_workspace(&self, id: WorkspaceId) -> Result<Workspace> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, dept_id, name, created_at, updated_at FROM workspaces WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let dept_s: String = row.get(1)?;
                let name: String = row.get(2)?;
                let ca: String = row.get(3)?;
                let ua: String = row.get(4)?;
                Ok(Workspace {
                    id: WorkspaceId(parse_uuid(&id_s)),
                    dept_id: DeptId(parse_uuid(&dept_s)),
                    name,
                    created_at: parse_ts(&ca),
                    updated_at: parse_ts(&ua),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Workspace {id} not found")))
    }

    fn list_workspaces_in_dept(&self, dept_id: DeptId) -> Vec<Workspace> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, dept_id, name, created_at, updated_at FROM workspaces WHERE dept_id = ?1")
            .unwrap();
        stmt.query_map(params![dept_id.0.to_string()], |row| {
            let id_s: String = row.get(0)?;
            let dept_s: String = row.get(1)?;
            let name: String = row.get(2)?;
            let ca: String = row.get(3)?;
            let ua: String = row.get(4)?;
            Ok(Workspace {
                id: WorkspaceId(parse_uuid(&id_s)),
                dept_id: DeptId(parse_uuid(&dept_s)),
                name,
                created_at: parse_ts(&ca),
                updated_at: parse_ts(&ua),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn list_workspaces_all(&self) -> Vec<Workspace> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, dept_id, name, created_at, updated_at FROM workspaces")
            .unwrap();
        stmt.query_map([], |row| {
            let id_s: String = row.get(0)?;
            let dept_s: String = row.get(1)?;
            let name: String = row.get(2)?;
            let ca: String = row.get(3)?;
            let ua: String = row.get(4)?;
            Ok(Workspace {
                id: WorkspaceId(parse_uuid(&id_s)),
                dept_id: DeptId(parse_uuid(&dept_s)),
                name,
                created_at: parse_ts(&ca),
                updated_at: parse_ts(&ua),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM workspaces WHERE id = ?1",
                params![id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete workspace: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Workspace {id} not found")));
        }
        Ok(())
    }

    // ── Document ────────────────────────────────────────────────────

    fn insert_document(&self, doc: Document) -> Result<Document> {
        self.get_workspace(doc.workspace_id)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                doc.id.0.to_string(),
                doc.workspace_id.0.to_string(),
                doc.title,
                doc.mime_type,
                doc.size_bytes,
                doc.status.to_string(),
                doc.chunk_count,
                doc.error_message,
                doc.processing_step,
                ts(&doc.created_at),
                ts(&doc.updated_at),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert document: {e}")))?;
        Ok(doc)
    }

    fn get_document(&self, id: DocId) -> Result<Document> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, created_at, updated_at FROM documents WHERE id = ?1",
            params![id.0.to_string()],
            doc_from_row,
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, created_at, updated_at FROM documents WHERE workspace_id = ?1")
            .unwrap();
        stmt.query_map(params![workspace_id.0.to_string()], doc_from_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    fn update_document_status(
        &self,
        id: DocId,
        status: DocStatus,
        chunk_count: i64,
        error_message: Option<String>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE documents SET status = ?1, chunk_count = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5",
                params![
                    status.to_string(),
                    chunk_count,
                    error_message,
                    ts(&Utc::now()),
                    id.0.to_string(),
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update document status: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn update_document_step(&self, id: DocId, step: Option<String>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE documents SET processing_step = ?1, updated_at = ?2 WHERE id = ?3",
                params![step, ts(&Utc::now()), id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update document step: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn delete_document(&self, id: DocId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM documents WHERE id = ?1",
                params![id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete document: {e}")))?;
        if affected == 0 {
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
        let conn = self.conn.lock().unwrap();
        let now = ts(&Utc::now());
        conn.execute(
            "INSERT INTO document_blobs (doc_id, original_bytes, converted_text, image_count, table_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(doc_id) DO UPDATE SET original_bytes = ?2, converted_text = ?3, image_count = ?4, table_count = ?5",
            params![
                doc_id.0.to_string(),
                original_bytes,
                converted_text,
                image_count,
                table_count,
                now,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite save document blob: {e}")))?;
        Ok(())
    }

    fn get_document_content(&self, doc_id: DocId) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT converted_text FROM document_blobs WHERE doc_id = ?1",
            params![doc_id.0.to_string()],
            |row| row.get(0),
        )
        .map_err(|_| ThaiRagError::NotFound(format!("No blob for document {doc_id}")))
    }

    fn get_document_file(&self, doc_id: DocId) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT original_bytes FROM document_blobs WHERE doc_id = ?1",
            params![doc_id.0.to_string()],
            |row| row.get(0),
        )
        .map_err(|_| ThaiRagError::NotFound(format!("No blob for document {doc_id}")))
    }

    fn get_document_blob_stats(&self, doc_id: DocId) -> Result<(i32, i32)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT image_count, table_count FROM document_blobs WHERE doc_id = ?1",
            params![doc_id.0.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| ThaiRagError::NotFound(format!("No blob for document {doc_id}")))
    }

    // ── Document Chunks ────────────────────────────────────────────

    fn save_chunks(&self, chunks: &[thairag_core::types::DocumentChunk]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        for chunk in chunks {
            conn.execute(
                "INSERT OR REPLACE INTO document_chunks (chunk_id, doc_id, workspace_id, content, chunk_index)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    chunk.chunk_id.0.to_string(),
                    chunk.doc_id.0.to_string(),
                    chunk.workspace_id.0.to_string(),
                    chunk.content,
                    chunk.chunk_index as i32,
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite save chunk: {e}")))?;
        }
        Ok(())
    }

    fn load_all_chunks(&self) -> Vec<thairag_core::types::DocumentChunk> {
        use thairag_core::types::{ChunkId, DocumentChunk, WorkspaceId};
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT chunk_id, doc_id, workspace_id, content, chunk_index FROM document_chunks",
            )
            .unwrap();
        stmt.query_map([], |row| {
            let chunk_id_str: String = row.get(0)?;
            let doc_id_str: String = row.get(1)?;
            let ws_id_str: String = row.get(2)?;
            let content: String = row.get(3)?;
            let chunk_index: i32 = row.get(4)?;
            Ok(DocumentChunk {
                chunk_id: ChunkId(chunk_id_str.parse().unwrap_or_default()),
                doc_id: DocId(doc_id_str.parse().unwrap_or_default()),
                workspace_id: WorkspaceId(ws_id_str.parse().unwrap_or_default()),
                content,
                chunk_index: chunk_index as usize,
                embedding: None,
                metadata: None,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_chunks_by_doc(&self, doc_id: DocId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM document_chunks WHERE doc_id = ?1",
            params![doc_id.0.to_string()],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite delete chunks: {e}")))?;
        Ok(())
    }

    // ── User ──────────────────────────────────────────────────────────

    fn insert_user(&self, email: String, name: String, password_hash: String) -> Result<User> {
        let email_lower = email.to_lowercase();
        let conn = self.conn.lock().unwrap();

        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE email = ?1",
                params![email_lower],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if exists {
            return Err(ThaiRagError::Validation(format!(
                "Email {email} is already registered"
            )));
        }

        let user = User {
            id: UserId::new(),
            email: email_lower.clone(),
            name,
            auth_provider: "local".into(),
            external_id: None,
            is_super_admin: false,
            role: "viewer".into(),
            created_at: Utc::now(),
        };
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                user.id.0.to_string(),
                user.email,
                user.name,
                password_hash,
                user.auth_provider,
                user.external_id,
                user.is_super_admin as i32,
                user.role,
                ts(&user.created_at),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert user: {e}")))?;
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
        let conn = self.conn.lock().unwrap();

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM users WHERE email = ?1",
                params![email_lower],
                |row| row.get(0),
            )
            .ok();

        if let Some(id_s) = existing {
            conn.execute(
                "UPDATE users SET name = ?1, password_hash = ?2, is_super_admin = ?3, role = ?4 WHERE id = ?5",
                params![name, password_hash, is_super_admin as i32, role, id_s],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite upsert user: {e}")))?;
            drop(conn);
            return self.get_user(UserId(parse_uuid(&id_s)));
        }

        let user = User {
            id: UserId::new(),
            email: email_lower.clone(),
            name,
            auth_provider: "local".into(),
            external_id: None,
            is_super_admin,
            role,
            created_at: Utc::now(),
        };
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                user.id.0.to_string(),
                user.email,
                user.name,
                password_hash,
                user.auth_provider,
                user.external_id,
                user.is_super_admin as i32,
                user.role,
                ts(&user.created_at),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite upsert user insert: {e}")))?;
        Ok(user)
    }

    fn delete_user(&self, id: UserId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute("DELETE FROM users WHERE id = ?1", params![id.0.to_string()])
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete user: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("User {id} not found")));
        }
        Ok(())
    }

    fn get_user_by_email(&self, email: &str) -> Result<UserRecord> {
        let email_lower = email.to_lowercase();
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, created_at FROM users WHERE email = ?1",
            params![email_lower],
            |row| {
                let id_s: String = row.get(0)?;
                let email: String = row.get(1)?;
                let name: String = row.get(2)?;
                let pw: String = row.get(3)?;
                let auth_provider: String = row.get(4)?;
                let external_id: Option<String> = row.get(5)?;
                let is_super_admin: i32 = row.get(6)?;
                let role: String = row.get(7)?;
                let ca: String = row.get(8)?;
                Ok(UserRecord {
                    user: User {
                        id: UserId(parse_uuid(&id_s)),
                        email,
                        name,
                        auth_provider,
                        external_id,
                        is_super_admin: is_super_admin != 0,
                        role,
                        created_at: parse_ts(&ca),
                    },
                    password_hash: pw,
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("User with email {email} not found")))
    }

    fn get_user(&self, id: UserId) -> Result<User> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, name, auth_provider, external_id, is_super_admin, role, created_at FROM users WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let email: String = row.get(1)?;
                let name: String = row.get(2)?;
                let auth_provider: String = row.get(3)?;
                let external_id: Option<String> = row.get(4)?;
                let is_super_admin: i32 = row.get(5)?;
                let role: String = row.get(6)?;
                let ca: String = row.get(7)?;
                Ok(User {
                    id: UserId(parse_uuid(&id_s)),
                    email,
                    name,
                    auth_provider,
                    external_id,
                    is_super_admin: is_super_admin != 0,
                    role,
                    created_at: parse_ts(&ca),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("User {id} not found")))
    }

    fn list_users(&self) -> Vec<User> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, email, name, auth_provider, external_id, is_super_admin, role, created_at FROM users")
            .unwrap();
        stmt.query_map([], |row| {
            let id_s: String = row.get(0)?;
            let email: String = row.get(1)?;
            let name: String = row.get(2)?;
            let auth_provider: String = row.get(3)?;
            let external_id: Option<String> = row.get(4)?;
            let is_super_admin: i32 = row.get(5)?;
            let role: String = row.get(6)?;
            let ca: String = row.get(7)?;
            Ok(User {
                id: UserId(parse_uuid(&id_s)),
                email,
                name,
                auth_provider,
                external_id,
                is_super_admin: is_super_admin != 0,
                role,
                created_at: parse_ts(&ca),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── Identity Providers ──────────────────────────────────────────

    fn list_identity_providers(&self) -> Vec<IdentityProvider> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, provider_type, enabled, config_json, created_at, updated_at FROM identity_providers")
            .unwrap();
        stmt.query_map([], |row| {
            let id_s: String = row.get(0)?;
            let name: String = row.get(1)?;
            let pt: String = row.get(2)?;
            let enabled: i32 = row.get(3)?;
            let config_json: String = row.get(4)?;
            let ca: String = row.get(5)?;
            let ua: String = row.get(6)?;
            Ok(IdentityProvider {
                id: IdpId(parse_uuid(&id_s)),
                name,
                provider_type: pt,
                enabled: enabled != 0,
                config: serde_json::from_str(&config_json).unwrap_or_default(),
                created_at: parse_ts(&ca),
                updated_at: parse_ts(&ua),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn list_enabled_identity_providers(&self) -> Vec<IdentityProvider> {
        self.list_identity_providers()
            .into_iter()
            .filter(|p| p.enabled)
            .collect()
    }

    fn get_identity_provider(&self, id: IdpId) -> Result<IdentityProvider> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, provider_type, enabled, config_json, created_at, updated_at FROM identity_providers WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let name: String = row.get(1)?;
                let pt: String = row.get(2)?;
                let enabled: i32 = row.get(3)?;
                let config_json: String = row.get(4)?;
                let ca: String = row.get(5)?;
                let ua: String = row.get(6)?;
                Ok(IdentityProvider {
                    id: IdpId(parse_uuid(&id_s)),
                    name,
                    provider_type: pt,
                    enabled: enabled != 0,
                    config: serde_json::from_str(&config_json).unwrap_or_default(),
                    created_at: parse_ts(&ca),
                    updated_at: parse_ts(&ua),
                })
            },
        )
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO identity_providers (id, name, provider_type, enabled, config_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                idp.id.0.to_string(),
                idp.name,
                idp.provider_type,
                idp.enabled as i32,
                serde_json::to_string(&config).unwrap_or_default(),
                ts(&now),
                ts(&now),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert idp: {e}")))?;
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
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE identity_providers SET name = ?1, provider_type = ?2, enabled = ?3, config_json = ?4, updated_at = ?5 WHERE id = ?6",
                params![
                    name,
                    provider_type,
                    enabled as i32,
                    serde_json::to_string(&config).unwrap_or_default(),
                    ts(&now),
                    id.0.to_string(),
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update idp: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Identity provider {id} not found"
            )));
        }
        drop(conn);
        self.get_identity_provider(id)
    }

    fn delete_identity_provider(&self, id: IdpId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM identity_providers WHERE id = ?1",
                params![id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete idp: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Identity provider {id} not found"
            )));
        }
        Ok(())
    }

    // ── Permissions ─────────────────────────────────────────────────

    fn add_permission(&self, perm: UserPermission) {
        let conn = self.conn.lock().unwrap();
        let (level, org_id, dept_id, ws_id) = scope_to_parts(&perm.scope);
        let _ = conn.execute(
            "INSERT INTO permissions (user_id, scope_level, org_id, dept_id, workspace_id, role) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![perm.user_id.0.to_string(), level, org_id, dept_id, ws_id, role_str(&perm.role)],
        );
    }

    fn upsert_permission(&self, perm: UserPermission) -> bool {
        let conn = self.conn.lock().unwrap();
        let (level, org_id, dept_id, ws_id) = scope_to_parts(&perm.scope);
        let role = role_str(&perm.role);

        let updated = conn
            .execute(
                "UPDATE permissions SET role = ?1 WHERE user_id = ?2 AND scope_level = ?3 AND org_id = ?4 AND dept_id = ?5 AND workspace_id = ?6",
                params![role, perm.user_id.0.to_string(), level, org_id, dept_id, ws_id],
            )
            .unwrap_or(0);

        if updated > 0 {
            return true;
        }

        let _ = conn.execute(
            "INSERT INTO permissions (user_id, scope_level, org_id, dept_id, workspace_id, role) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![perm.user_id.0.to_string(), level, org_id, dept_id, ws_id, role],
        );
        false
    }

    fn list_permissions_for_org(&self, org_id: OrgId) -> Vec<UserPermission> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT user_id, scope_level, org_id, dept_id, workspace_id, role FROM permissions WHERE org_id = ?1")
            .unwrap();
        stmt.query_map(params![org_id.0.to_string()], |row| {
            let uid: String = row.get(0)?;
            let level: String = row.get(1)?;
            let oid: String = row.get(2)?;
            let did: String = row.get(3)?;
            let wid: String = row.get(4)?;
            let role: String = row.get(5)?;
            Ok(UserPermission {
                user_id: UserId(parse_uuid(&uid)),
                scope: parts_to_scope(&level, &oid, &did, &wid),
                role: parse_role(&role),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn remove_permission(&self, user_id: UserId, scope: &PermissionScope) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let (level, org_id, dept_id, ws_id) = scope_to_parts(scope);
        let affected = conn
            .execute(
                "DELETE FROM permissions WHERE user_id = ?1 AND scope_level = ?2 AND org_id = ?3 AND dept_id = ?4 AND workspace_id = ?5",
                params![user_id.0.to_string(), level, org_id, dept_id, ws_id],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite remove permission: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound("Permission not found".into()));
        }
        Ok(())
    }

    fn count_org_owners(&self, org_id: OrgId) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM permissions WHERE org_id = ?1 AND scope_level = 'org' AND role = 'owner'",
            params![org_id.0.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    fn get_user_role_for_org(&self, user_id: UserId, org_id: OrgId) -> Option<Role> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT role FROM permissions WHERE user_id = ?1 AND org_id = ?2")
            .unwrap();
        let roles: Vec<Role> = stmt
            .query_map(
                params![user_id.0.to_string(), org_id.0.to_string()],
                |row| {
                    let r: String = row.get(0)?;
                    Ok(parse_role(&r))
                },
            )
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        roles.into_iter().max()
    }

    fn get_user_role_for_dept(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
    ) -> Option<Role> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT role FROM permissions WHERE user_id = ?1 AND org_id = ?2 \
                 AND ((scope_level = 'Org') OR (scope_level = 'Dept' AND dept_id = ?3))",
            )
            .unwrap();
        let roles: Vec<Role> = stmt
            .query_map(
                params![
                    user_id.0.to_string(),
                    org_id.0.to_string(),
                    dept_id.0.to_string()
                ],
                |row| {
                    let r: String = row.get(0)?;
                    Ok(parse_role(&r))
                },
            )
            .unwrap()
            .filter_map(|r| r.ok())
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT role FROM permissions WHERE user_id = ?1 AND org_id = ?2 \
                 AND ((scope_level = 'Org') \
                  OR (scope_level = 'Dept' AND dept_id = ?3) \
                  OR (scope_level = 'Workspace' AND dept_id = ?3 AND workspace_id = ?4))",
            )
            .unwrap();
        let roles: Vec<Role> = stmt
            .query_map(
                params![
                    user_id.0.to_string(),
                    org_id.0.to_string(),
                    dept_id.0.to_string(),
                    workspace_id.0.to_string()
                ],
                |row| {
                    let r: String = row.get(0)?;
                    Ok(parse_role(&r))
                },
            )
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        roles.into_iter().max()
    }

    fn list_user_permissions(&self, user_id: UserId) -> Vec<UserPermission> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT scope_level, org_id, dept_id, workspace_id, role \
                 FROM permissions WHERE user_id = ?1",
            )
            .unwrap();
        stmt.query_map(params![user_id.0.to_string()], |row| {
            let level: String = row.get(0)?;
            let oid: String = row.get(1)?;
            let did: String = row.get(2)?;
            let wid: String = row.get(3)?;
            let role_str: String = row.get(4)?;
            Ok(UserPermission {
                user_id,
                scope: parts_to_scope(&level, &oid, &did, &wid),
                role: parse_role(&role_str),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_user_workspace_ids(&self, user_id: UserId) -> Vec<WorkspaceId> {
        let perms = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT scope_level, org_id, dept_id, workspace_id FROM permissions WHERE user_id = ?1")
                .unwrap();
            stmt.query_map(params![user_id.0.to_string()], |row| {
                let level: String = row.get(0)?;
                let oid: String = row.get(1)?;
                let did: String = row.get(2)?;
                let wid: String = row.get(3)?;
                Ok(parts_to_scope(&level, &oid, &did, &wid))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>()
        };

        let mut ws_ids = Vec::new();
        for scope in &perms {
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM workspaces WHERE dept_id = ?1")
            .unwrap();
        stmt.query_map(params![dept_id.0.to_string()], |row| {
            let s: String = row.get(0)?;
            Ok(WorkspaceId(parse_uuid(&s)))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn dept_ids_in_org(&self, org_id: OrgId) -> Vec<DeptId> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM departments WHERE org_id = ?1")
            .unwrap();
        stmt.query_map(params![org_id.0.to_string()], |row| {
            let s: String = row.get(0)?;
            Ok(DeptId(parse_uuid(&s)))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn doc_ids_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM documents WHERE workspace_id = ?1")
            .unwrap();
        stmt.query_map(params![workspace_id.0.to_string()], |row| {
            let s: String = row.get(0)?;
            Ok(DocId(parse_uuid(&s)))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn cascade_delete_workspace_docs(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        let doc_ids = self.doc_ids_in_workspace(workspace_id);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM documents WHERE workspace_id = ?1",
            params![workspace_id.0.to_string()],
        )
        .ok();
        doc_ids
    }

    fn cascade_delete_workspace(&self, ws_id: WorkspaceId) -> Result<Vec<DocId>> {
        let doc_ids = self.cascade_delete_workspace_docs(ws_id);
        // CASCADE will handle child deletions; delete permissions manually
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM permissions WHERE scope_level = 'workspace' AND workspace_id = ?1",
            params![ws_id.0.to_string()],
        )
        .ok();
        drop(conn);
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
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM permissions WHERE scope_level = 'dept' AND dept_id = ?1",
                params![dept_id.0.to_string()],
            )
            .ok();
            for ws_id in &ws_ids {
                conn.execute(
                    "DELETE FROM permissions WHERE scope_level = 'workspace' AND workspace_id = ?1",
                    params![ws_id.0.to_string()],
                )
                .ok();
            }
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
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM permissions WHERE org_id = ?1",
                params![org_id.0.to_string()],
            )
            .ok();
        }
        // CASCADE handles children
        self.delete_org(org_id)?;
        Ok(all_doc_ids)
    }

    fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .ok()
    }

    fn set_setting(&self, key: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            params![key, value, now],
        )
        .ok();
    }

    fn delete_setting(&self, key: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
            .ok();
    }

    fn list_all_settings(&self) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE key NOT LIKE 'snapshot.%' AND key NOT LIKE '\\_snapshot\\_index%' ESCAPE '\\' AND key NOT LIKE '\\_embedding\\_fingerprint%' ESCAPE '\\'")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── MCP Connectors ───────────────────────────────────────────────

    fn insert_connector(&self, config: McpConnectorConfig) -> Result<McpConnectorConfig> {
        self.get_workspace(config.workspace_id)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mcp_connectors (id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                config.id.0.to_string(),
                config.name,
                config.description,
                serde_json::to_string(&config.transport).unwrap_or_default().trim_matches('"'),
                config.command,
                serde_json::to_string(&config.args).unwrap_or_default(),
                serde_json::to_string(&config.env).unwrap_or_default(),
                config.url,
                serde_json::to_string(&config.headers).unwrap_or_default(),
                config.workspace_id.0.to_string(),
                serde_json::to_string(&config.sync_mode).unwrap_or_default().trim_matches('"'),
                config.schedule_cron,
                serde_json::to_string(&config.resource_filters).unwrap_or_default(),
                config.max_items_per_sync.map(|v| v as i64),
                serde_json::to_string(&config.tool_calls).unwrap_or_default(),
                config.webhook_url,
                config.webhook_secret,
                serde_json::to_string(&config.status).unwrap_or_default().trim_matches('"'),
                ts(&config.created_at),
                ts(&config.updated_at),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert connector: {e}")))?;
        Ok(config)
    }

    fn get_connector(&self, id: ConnectorId) -> Result<McpConnectorConfig> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at FROM mcp_connectors WHERE id = ?1",
            params![id.0.to_string()],
            connector_from_row,
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Connector {id} not found")))
    }

    fn list_connectors(&self) -> Vec<McpConnectorConfig> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at FROM mcp_connectors")
            .unwrap();
        stmt.query_map([], connector_from_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    fn list_connectors_for_workspace(&self, ws_id: WorkspaceId) -> Vec<McpConnectorConfig> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, description, transport, command, args, env, url, headers, workspace_id, sync_mode, schedule_cron, resource_filters, max_items_per_sync, tool_calls, webhook_url, webhook_secret, status, created_at, updated_at FROM mcp_connectors WHERE workspace_id = ?1")
            .unwrap();
        stmt.query_map(params![ws_id.0.to_string()], connector_from_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    fn update_connector(&self, config: McpConnectorConfig) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE mcp_connectors SET name = ?1, description = ?2, transport = ?3, command = ?4, args = ?5, env = ?6, url = ?7, headers = ?8, workspace_id = ?9, sync_mode = ?10, schedule_cron = ?11, resource_filters = ?12, max_items_per_sync = ?13, tool_calls = ?14, webhook_url = ?15, webhook_secret = ?16, status = ?17, updated_at = ?18 WHERE id = ?19",
                params![
                    config.name,
                    config.description,
                    serde_json::to_string(&config.transport).unwrap_or_default().trim_matches('"'),
                    config.command,
                    serde_json::to_string(&config.args).unwrap_or_default(),
                    serde_json::to_string(&config.env).unwrap_or_default(),
                    config.url,
                    serde_json::to_string(&config.headers).unwrap_or_default(),
                    config.workspace_id.0.to_string(),
                    serde_json::to_string(&config.sync_mode).unwrap_or_default().trim_matches('"'),
                    config.schedule_cron,
                    serde_json::to_string(&config.resource_filters).unwrap_or_default(),
                    config.max_items_per_sync.map(|v| v as i64),
                    serde_json::to_string(&config.tool_calls).unwrap_or_default(),
                    config.webhook_url,
                    config.webhook_secret,
                    serde_json::to_string(&config.status).unwrap_or_default().trim_matches('"'),
                    ts(&config.updated_at),
                    config.id.0.to_string(),
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update connector: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Connector {} not found",
                config.id
            )));
        }
        Ok(())
    }

    fn delete_connector(&self, id: ConnectorId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM mcp_connectors WHERE id = ?1",
                params![id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete connector: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Connector {id} not found")));
        }
        Ok(())
    }

    fn update_connector_status(&self, id: ConnectorId, status: ConnectorStatus) -> Result<()> {
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE mcp_connectors SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    serde_json::to_string(&status)
                        .unwrap_or_default()
                        .trim_matches('"'),
                    ts(&now),
                    id.0.to_string(),
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update connector status: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Connector {id} not found")));
        }
        Ok(())
    }

    // ── MCP Sync State ───────────────────────────────────────────────

    fn get_sync_state(&self, connector_id: ConnectorId, resource_uri: &str) -> Option<SyncState> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT connector_id, resource_uri, content_hash, doc_id, last_synced_at, source_metadata FROM mcp_sync_states WHERE connector_id = ?1 AND resource_uri = ?2",
            params![connector_id.0.to_string(), resource_uri],
            sync_state_from_row,
        )
        .ok()
    }

    fn upsert_sync_state(&self, state: SyncState) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mcp_sync_states (connector_id, resource_uri, content_hash, doc_id, last_synced_at, source_metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(connector_id, resource_uri) DO UPDATE SET content_hash = ?3, doc_id = ?4, last_synced_at = ?5, source_metadata = ?6",
            params![
                state.connector_id.0.to_string(),
                state.resource_uri,
                state.content_hash,
                state.doc_id.map(|d| d.0.to_string()),
                ts(&state.last_synced_at),
                state.source_metadata.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite upsert sync state: {e}")))?;
        Ok(())
    }

    fn list_sync_states(&self, connector_id: ConnectorId) -> Vec<SyncState> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT connector_id, resource_uri, content_hash, doc_id, last_synced_at, source_metadata FROM mcp_sync_states WHERE connector_id = ?1")
            .unwrap();
        stmt.query_map(params![connector_id.0.to_string()], |row| {
            sync_state_from_row(row)
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_sync_states(&self, connector_id: ConnectorId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM mcp_sync_states WHERE connector_id = ?1",
            params![connector_id.0.to_string()],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite delete sync states: {e}")))?;
        Ok(())
    }

    // ── MCP Sync Runs ────────────────────────────────────────────────

    fn insert_sync_run(&self, run: SyncRun) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mcp_sync_runs (id, connector_id, started_at, completed_at, status, items_discovered, items_created, items_updated, items_skipped, items_failed, error_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                run.id.0.to_string(),
                run.connector_id.0.to_string(),
                ts(&run.started_at),
                run.completed_at.as_ref().map(ts),
                serde_json::to_string(&run.status).unwrap_or_default().trim_matches('"'),
                run.items_discovered as i64,
                run.items_created as i64,
                run.items_updated as i64,
                run.items_skipped as i64,
                run.items_failed as i64,
                run.error_message,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert sync run: {e}")))?;
        Ok(())
    }

    fn update_sync_run(&self, run: SyncRun) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE mcp_sync_runs SET completed_at = ?1, status = ?2, items_discovered = ?3, items_created = ?4, items_updated = ?5, items_skipped = ?6, items_failed = ?7, error_message = ?8 WHERE id = ?9",
                params![
                    run.completed_at.as_ref().map(ts),
                    serde_json::to_string(&run.status).unwrap_or_default().trim_matches('"'),
                    run.items_discovered as i64,
                    run.items_created as i64,
                    run.items_updated as i64,
                    run.items_skipped as i64,
                    run.items_failed as i64,
                    run.error_message,
                    run.id.0.to_string(),
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update sync run: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Sync run {} not found",
                run.id
            )));
        }
        Ok(())
    }

    fn list_sync_runs(&self, connector_id: ConnectorId, limit: usize) -> Vec<SyncRun> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, connector_id, started_at, completed_at, status, items_discovered, items_created, items_updated, items_skipped, items_failed, error_message FROM mcp_sync_runs WHERE connector_id = ?1 ORDER BY started_at DESC LIMIT ?2")
            .unwrap();
        stmt.query_map(params![connector_id.0.to_string(), limit as i64], |row| {
            sync_run_from_row(row)
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_latest_sync_run(&self, connector_id: ConnectorId) -> Option<SyncRun> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, connector_id, started_at, completed_at, status, items_discovered, items_created, items_updated, items_skipped, items_failed, error_message FROM mcp_sync_runs WHERE connector_id = ?1 ORDER BY started_at DESC LIMIT 1",
            params![connector_id.0.to_string()],
            sync_run_from_row,
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::models::PermissionScope;
    use thairag_core::permission::Role;

    fn mem_store() -> SqliteKmStore {
        SqliteKmStore::new(":memory:").unwrap()
    }

    #[test]
    fn org_crud_roundtrip() {
        let store = mem_store();
        let org = store.insert_org("Acme Corp".into()).unwrap();
        assert_eq!(store.get_org(org.id).unwrap().name, "Acme Corp");
        assert_eq!(store.list_orgs().len(), 1);
        store.delete_org(org.id).unwrap();
        assert!(store.get_org(org.id).is_err());
    }

    #[test]
    fn dept_requires_valid_org() {
        let store = mem_store();
        let result = store.insert_dept(OrgId::new(), "Engineering".into());
        assert!(result.is_err());
    }

    #[test]
    fn dept_crud_roundtrip() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Engineering".into()).unwrap();
        assert_eq!(store.get_dept(dept.id).unwrap().name, "Engineering");
        assert_eq!(store.list_depts_in_org(org.id).len(), 1);
        store.delete_dept(dept.id).unwrap();
        assert!(store.get_dept(dept.id).is_err());
    }

    #[test]
    fn workspace_crud_roundtrip() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
        assert_eq!(store.get_workspace(ws.id).unwrap().name, "Main");
        assert_eq!(store.list_workspaces_in_dept(dept.id).len(), 1);
        store.delete_workspace(ws.id).unwrap();
        assert!(store.get_workspace(ws.id).is_err());
    }

    #[test]
    fn document_crud_roundtrip() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
        let now = Utc::now();
        let doc = Document {
            id: DocId::new(),
            workspace_id: ws.id,
            title: "readme".into(),
            mime_type: "text/plain".into(),
            size_bytes: 42,
            status: DocStatus::Ready,
            chunk_count: 0,
            error_message: None,
            processing_step: None,
            created_at: now,
            updated_at: now,
        };
        let doc = store.insert_document(doc).unwrap();
        assert_eq!(store.get_document(doc.id).unwrap().title, "readme");
        assert_eq!(store.list_documents_in_workspace(ws.id).len(), 1);
        store.delete_document(doc.id).unwrap();
        assert!(store.get_document(doc.id).is_err());
    }

    #[test]
    fn user_crud_roundtrip() {
        let store = mem_store();
        let user = store
            .insert_user("Alice@Example.com".into(), "Alice".into(), "hash123".into())
            .unwrap();
        assert_eq!(user.email, "alice@example.com");
        assert_eq!(user.auth_provider, "local");
        assert!(!user.is_super_admin);

        let record = store.get_user_by_email("alice@example.com").unwrap();
        assert_eq!(record.user.id, user.id);
        assert_eq!(record.password_hash, "hash123");

        let fetched = store.get_user(user.id).unwrap();
        assert_eq!(fetched.name, "Alice");
    }

    #[test]
    fn upsert_user_by_email_creates_and_updates() {
        let store = mem_store();
        let user = store
            .upsert_user_by_email(
                "admin@test.com".into(),
                "Admin".into(),
                "h1".into(),
                true,
                "super_admin".into(),
            )
            .unwrap();
        assert!(user.is_super_admin);
        assert_eq!(user.role, "super_admin");
        assert_eq!(user.email, "admin@test.com");

        let updated = store
            .upsert_user_by_email(
                "admin@test.com".into(),
                "Admin Updated".into(),
                "h2".into(),
                true,
                "super_admin".into(),
            )
            .unwrap();
        assert_eq!(updated.id, user.id);
        assert_eq!(updated.name, "Admin Updated");
    }

    #[test]
    fn delete_user_works() {
        let store = mem_store();
        let user = store
            .insert_user("del@test.com".into(), "Del".into(), "h".into())
            .unwrap();
        store.delete_user(user.id).unwrap();
        assert!(store.get_user(user.id).is_err());
    }

    #[test]
    fn identity_provider_crud() {
        let store = mem_store();
        let idp = store
            .insert_identity_provider(
                "Google".into(),
                "oidc".into(),
                true,
                serde_json::json!({"issuer_url": "https://accounts.google.com"}),
            )
            .unwrap();
        assert_eq!(idp.name, "Google");
        assert!(idp.enabled);

        let fetched = store.get_identity_provider(idp.id).unwrap();
        assert_eq!(fetched.provider_type, "oidc");

        let updated = store
            .update_identity_provider(
                idp.id,
                "Google SSO".into(),
                "oidc".into(),
                false,
                serde_json::json!({}),
            )
            .unwrap();
        assert_eq!(updated.name, "Google SSO");
        assert!(!updated.enabled);

        assert_eq!(store.list_identity_providers().len(), 1);
        assert_eq!(store.list_enabled_identity_providers().len(), 0);

        store.delete_identity_provider(idp.id).unwrap();
        assert!(store.get_identity_provider(idp.id).is_err());
    }

    #[test]
    fn user_email_uniqueness() {
        let store = mem_store();
        store
            .insert_user("bob@test.com".into(), "Bob".into(), "h".into())
            .unwrap();
        let result = store.insert_user("BOB@test.com".into(), "Bob2".into(), "h2".into());
        assert!(result.is_err());
    }

    #[test]
    fn permission_resolution() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();

        let user = store
            .insert_user("u@test.com".into(), "U".into(), "h".into())
            .unwrap();

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Viewer,
        });

        assert_eq!(
            store.get_user_role_for_org(user.id, org.id),
            Some(Role::Viewer)
        );

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Workspace {
                org_id: org.id,
                dept_id: dept.id,
                workspace_id: ws.id,
            },
            role: Role::Editor,
        });

        assert_eq!(
            store.get_user_role_for_org(user.id, org.id),
            Some(Role::Editor)
        );

        let ws_ids = store.get_user_workspace_ids(user.id);
        assert!(ws_ids.contains(&ws.id));
    }

    #[test]
    fn upsert_permission_dedup() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let user = store
            .insert_user("u@t.com".into(), "U".into(), "h".into())
            .unwrap();
        let scope = PermissionScope::Org { org_id: org.id };

        let updated = store.upsert_permission(UserPermission {
            user_id: user.id,
            scope: scope.clone(),
            role: Role::Viewer,
        });
        assert!(!updated);
        assert_eq!(store.list_permissions_for_org(org.id).len(), 1);

        let updated = store.upsert_permission(UserPermission {
            user_id: user.id,
            scope: scope.clone(),
            role: Role::Editor,
        });
        assert!(updated);
        let perms = store.list_permissions_for_org(org.id);
        assert_eq!(perms.len(), 1);
        assert_eq!(perms[0].role, Role::Editor);
    }

    #[test]
    fn cascade_delete_org() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
        let now = Utc::now();
        let doc = Document {
            id: DocId::new(),
            workspace_id: ws.id,
            title: "test".into(),
            mime_type: "text/plain".into(),
            size_bytes: 10,
            status: DocStatus::Ready,
            chunk_count: 0,
            error_message: None,
            processing_step: None,
            created_at: now,
            updated_at: now,
        };
        let doc = store.insert_document(doc).unwrap();

        let deleted = store.cascade_delete_org(org.id).unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0], doc.id);
        assert!(store.get_org(org.id).is_err());
        assert!(store.get_dept(dept.id).is_err());
        assert!(store.get_workspace(ws.id).is_err());
        assert!(store.get_document(doc.id).is_err());
    }

    #[test]
    fn count_org_owners() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let user = store
            .insert_user("o@t.com".into(), "O".into(), "h".into())
            .unwrap();

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Owner,
        });

        assert_eq!(store.count_org_owners(org.id), 1);
    }
}
