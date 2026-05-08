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
    AclPermission, ApiKeyId, ConnectorId, ConnectorStatus, DeptId, DocId, DocumentAcl, IdpId,
    McpConnectorConfig, McpTransport, OrgId, SyncMode, SyncRun, SyncRunId, SyncRunStatus,
    SyncState, UserId, WorkspaceAcl, WorkspaceId,
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
            "ALTER TABLE documents ADD COLUMN version INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE documents ADD COLUMN content_hash TEXT",
            "ALTER TABLE documents ADD COLUMN source_url TEXT",
            "ALTER TABLE documents ADD COLUMN refresh_schedule TEXT",
            "ALTER TABLE documents ADD COLUMN last_refreshed_at TEXT",
            "ALTER TABLE users ADD COLUMN disabled INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE finetune_jobs ADD COLUMN config TEXT",
        ] {
            let _ = conn.execute_batch(stmt); // ignore "duplicate column" errors
        }

        // Fix existing super admins that have default 'viewer' role
        let _ = conn.execute_batch(
            "UPDATE users SET role = 'super_admin' WHERE is_super_admin = 1 AND role = 'viewer'",
        );

        // Migrate settings table: add scope_type + scope_id columns for multi-tenant support.
        // Check if the old single-column PK schema is in use and migrate to composite PK.
        let needs_settings_migration: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('settings') WHERE name = 'scope_type'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(1)
            == 0;
        if needs_settings_migration {
            conn.execute_batch(
                "CREATE TABLE settings_v2 (
                    key         TEXT NOT NULL,
                    scope_type  TEXT NOT NULL DEFAULT 'global',
                    scope_id    TEXT NOT NULL DEFAULT '',
                    value       TEXT NOT NULL,
                    updated_at  TEXT NOT NULL,
                    PRIMARY KEY (key, scope_type, scope_id)
                );
                INSERT OR IGNORE INTO settings_v2 (key, scope_type, scope_id, value, updated_at)
                    SELECT key, 'global', '', value, updated_at FROM settings;
                DROP TABLE settings;
                ALTER TABLE settings_v2 RENAME TO settings;",
            )
            .map_err(|e| ThaiRagError::Config(format!("Settings migration failed: {e}")))?;
        }

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
    let version: i32 = row.get(9)?;
    let content_hash: Option<String> = row.get(10)?;
    let source_url: Option<String> = row.get(11)?;
    let refresh_schedule: Option<String> = row.get(12)?;
    let last_refreshed_at_s: Option<String> = row.get(13)?;
    let ca: String = row.get(14)?;
    let ua: String = row.get(15)?;
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
        version,
        content_hash,
        source_url,
        refresh_schedule,
        last_refreshed_at: last_refreshed_at_s.map(|s| parse_ts(&s)),
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
            "INSERT INTO documents (id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                doc.version,
                doc.content_hash,
                doc.source_url,
                doc.refresh_schedule,
                doc.last_refreshed_at.map(|dt| ts(&dt)),
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
            "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at FROM documents WHERE id = ?1",
            params![id.0.to_string()],
            doc_from_row,
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at FROM documents WHERE workspace_id = ?1")
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

    fn update_document_version_info(
        &self,
        id: DocId,
        version: i32,
        content_hash: Option<String>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE documents SET version = ?1, content_hash = ?2, updated_at = ?3 WHERE id = ?4",
                params![version, content_hash, ts(&Utc::now()), id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update document version info: {e}")))?;
        if affected == 0 {
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
        let conn = self.conn.lock().unwrap();
        let next_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version_number), 0) + 1 FROM document_versions WHERE doc_id = ?1",
                params![doc_id.0.to_string()],
                |row| row.get(0),
            )
            .unwrap_or(1);

        let id = Uuid::new_v4().to_string();
        let now = ts(&Utc::now());
        conn.execute(
            "INSERT INTO document_versions (id, doc_id, version_number, title, content, content_hash, mime_type, size_bytes, created_at, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                doc_id.0.to_string(),
                next_version,
                title,
                content,
                content_hash,
                mime_type,
                size_bytes,
                now,
                created_by.map(|u| u.0.to_string()),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite save document version: {e}")))?;

        Ok(super::DocumentVersion {
            id,
            doc_id,
            version_number: next_version,
            title: title.to_string(),
            content: content.map(|s| s.to_string()),
            content_hash: content_hash.to_string(),
            mime_type: mime_type.to_string(),
            size_bytes,
            created_at: now,
            created_by,
        })
    }

    fn list_document_versions(&self, doc_id: DocId) -> Vec<super::DocumentVersion> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, doc_id, version_number, title, content, content_hash, mime_type, size_bytes, created_at, created_by
                 FROM document_versions WHERE doc_id = ?1 ORDER BY version_number DESC",
            )
            .unwrap();
        stmt.query_map(params![doc_id.0.to_string()], |row| {
            let created_by_str: Option<String> = row.get(9)?;
            Ok(super::DocumentVersion {
                id: row.get(0)?,
                doc_id: DocId(parse_uuid(&row.get::<_, String>(1)?)),
                version_number: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                content_hash: row.get(5)?,
                mime_type: row.get(6)?,
                size_bytes: row.get(7)?,
                created_at: row.get(8)?,
                created_by: created_by_str.map(|s| UserId(parse_uuid(&s))),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_document_version(
        &self,
        doc_id: DocId,
        version_number: i32,
    ) -> Option<super::DocumentVersion> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, doc_id, version_number, title, content, content_hash, mime_type, size_bytes, created_at, created_by
             FROM document_versions WHERE doc_id = ?1 AND version_number = ?2",
            params![doc_id.0.to_string(), version_number],
            |row| {
                let created_by_str: Option<String> = row.get(9)?;
                Ok(super::DocumentVersion {
                    id: row.get(0)?,
                    doc_id: DocId(parse_uuid(&row.get::<_, String>(1)?)),
                    version_number: row.get(2)?,
                    title: row.get(3)?,
                    content: row.get(4)?,
                    content_hash: row.get(5)?,
                    mime_type: row.get(6)?,
                    size_bytes: row.get(7)?,
                    created_at: row.get(8)?,
                    created_by: created_by_str.map(|s| UserId(parse_uuid(&s))),
                })
            },
        )
        .ok()
    }

    // ── Document Refresh Schedule ────────────────────────────────

    fn update_document_schedule(
        &self,
        id: DocId,
        source_url: Option<String>,
        refresh_schedule: Option<String>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE documents SET source_url = ?1, refresh_schedule = ?2, updated_at = ?3 WHERE id = ?4",
                params![source_url, refresh_schedule, ts(&Utc::now()), id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite update document schedule: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn touch_document_refreshed(&self, id: DocId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = ts(&Utc::now());
        let affected = conn
            .execute(
                "UPDATE documents SET last_refreshed_at = ?1, updated_at = ?2 WHERE id = ?3",
                params![now, now, id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite touch document refreshed: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    fn list_documents_due_for_refresh(&self) -> Vec<Document> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, version, content_hash, source_url, refresh_schedule, last_refreshed_at, created_at, updated_at
                 FROM documents
                 WHERE status = 'ready' AND source_url IS NOT NULL AND refresh_schedule IS NOT NULL",
            )
            .unwrap();
        let all: Vec<Document> = stmt
            .query_map([], doc_from_row)
            .unwrap()
            .filter_map(|r| r.ok())
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
            disabled: false,
            created_at: Utc::now(),
        };
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, disabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                user.id.0.to_string(),
                user.email,
                user.name,
                password_hash,
                user.auth_provider,
                user.external_id,
                user.is_super_admin as i32,
                user.role,
                user.disabled as i32,
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
            disabled: false,
            created_at: Utc::now(),
        };
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, disabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                user.id.0.to_string(),
                user.email,
                user.name,
                password_hash,
                user.auth_provider,
                user.external_id,
                user.is_super_admin as i32,
                user.role,
                user.disabled as i32,
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
            "SELECT id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, disabled, created_at FROM users WHERE email = ?1",
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
                let disabled: i32 = row.get(8)?;
                let ca: String = row.get(9)?;
                Ok(UserRecord {
                    user: User {
                        id: UserId(parse_uuid(&id_s)),
                        email,
                        name,
                        auth_provider,
                        external_id,
                        is_super_admin: is_super_admin != 0,
                        role,
                        disabled: disabled != 0,
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
            "SELECT id, email, name, auth_provider, external_id, is_super_admin, role, disabled, created_at FROM users WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let email: String = row.get(1)?;
                let name: String = row.get(2)?;
                let auth_provider: String = row.get(3)?;
                let external_id: Option<String> = row.get(4)?;
                let is_super_admin: i32 = row.get(5)?;
                let role: String = row.get(6)?;
                let disabled: i32 = row.get(7)?;
                let ca: String = row.get(8)?;
                Ok(User {
                    id: UserId(parse_uuid(&id_s)),
                    email,
                    name,
                    auth_provider,
                    external_id,
                    is_super_admin: is_super_admin != 0,
                    role,
                    disabled: disabled != 0,
                    created_at: parse_ts(&ca),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("User {id} not found")))
    }

    fn list_users(&self) -> Vec<User> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, email, name, auth_provider, external_id, is_super_admin, role, disabled, created_at FROM users")
            .unwrap();
        stmt.query_map([], |row| {
            let id_s: String = row.get(0)?;
            let email: String = row.get(1)?;
            let name: String = row.get(2)?;
            let auth_provider: String = row.get(3)?;
            let external_id: Option<String> = row.get(4)?;
            let is_super_admin: i32 = row.get(5)?;
            let role: String = row.get(6)?;
            let disabled: i32 = row.get(7)?;
            let ca: String = row.get(8)?;
            Ok(User {
                id: UserId(parse_uuid(&id_s)),
                email,
                name,
                auth_provider,
                external_id,
                is_super_admin: is_super_admin != 0,
                role,
                disabled: disabled != 0,
                created_at: parse_ts(&ca),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn set_user_disabled(&self, id: UserId, disabled: bool) -> Result<User> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE users SET disabled = ?1 WHERE id = ?2",
                params![disabled as i32, id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Internal(format!("SQLite set_user_disabled: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("User {id} not found")));
        }
        drop(conn);
        self.get_user(id)
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
        self.get_scoped_setting(key, "global", "")
    }

    fn set_setting(&self, key: &str, value: &str) {
        self.set_scoped_setting(key, "global", "", value);
    }

    fn delete_setting(&self, key: &str) {
        self.delete_scoped_setting(key, "global", "");
    }

    fn list_all_settings(&self) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE scope_type = 'global' AND scope_id = '' AND key NOT LIKE 'snapshot.%' AND key NOT LIKE '\\_snapshot\\_index%' ESCAPE '\\' AND key NOT LIKE '\\_embedding\\_fingerprint%' ESCAPE '\\'")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1 AND scope_type = ?2 AND scope_id = ?3",
            params![key, scope_type, scope_id],
            |row| row.get(0),
        )
        .ok()
    }

    fn set_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO settings (key, scope_type, scope_id, value, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key, scope_type, scope_id) DO UPDATE SET value = ?4, updated_at = ?5",
            params![key, scope_type, scope_id, value, now],
        )
        .ok();
    }

    fn delete_scoped_setting(&self, key: &str, scope_type: &str, scope_id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM settings WHERE key = ?1 AND scope_type = ?2 AND scope_id = ?3",
            params![key, scope_type, scope_id],
        )
        .ok();
    }

    fn list_scoped_settings(&self, scope_type: &str, scope_id: &str) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT key, value FROM settings WHERE scope_type = ?1 AND scope_id = ?2 \
                 AND key NOT LIKE 'snapshot.%' \
                 AND key NOT LIKE '\\_snapshot\\_index%' ESCAPE '\\' \
                 AND key NOT LIKE '\\_embedding\\_fingerprint%' ESCAPE '\\'",
            )
            .unwrap();
        stmt.query_map(params![scope_type, scope_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn list_override_keys(&self, scope_type: &str, scope_id: &str) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key FROM settings WHERE scope_type = ?1 AND scope_id = ?2")
            .unwrap();
        stmt.query_map(params![scope_type, scope_id], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    fn delete_all_scoped_settings(&self, scope_type: &str, scope_id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM settings WHERE scope_type = ?1 AND scope_id = ?2",
            params![scope_type, scope_id],
        )
        .ok();
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

    // ── API Key Vault ───────────────────────────────────────────────

    fn list_vault_keys(&self) -> Vec<super::VaultKeyRow> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at FROM api_key_vault ORDER BY created_at DESC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(super::VaultKeyRow {
                id: row.get(0)?,
                name: row.get(1)?,
                provider: row.get(2)?,
                encrypted_key: row.get(3)?,
                key_prefix: row.get(4)?,
                key_suffix: row.get(5)?,
                base_url: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_vault_key(&self, id: &str) -> Option<super::VaultKeyRow> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at FROM api_key_vault WHERE id = ?1",
            params![id],
            |row| {
                Ok(super::VaultKeyRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider: row.get(2)?,
                    encrypted_key: row.get(3)?,
                    key_prefix: row.get(4)?,
                    key_suffix: row.get(5)?,
                    base_url: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .ok()
    }

    fn upsert_vault_key(&self, row: &super::VaultKeyRow) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO api_key_vault (id, name, provider, encrypted_key, key_prefix, key_suffix, base_url, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET name = ?2, provider = ?3, encrypted_key = ?4, key_prefix = ?5, key_suffix = ?6, base_url = ?7, updated_at = ?9",
            params![
                row.id,
                row.name,
                row.provider,
                row.encrypted_key,
                row.key_prefix,
                row.key_suffix,
                row.base_url,
                row.created_at,
                row.updated_at,
            ],
        )
        .ok();
    }

    fn delete_vault_key(&self, id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM api_key_vault WHERE id = ?1", params![id])
            .ok();
    }

    // ── LLM Profiles ────────────────────────────────────────────────

    fn list_llm_profiles(&self) -> Vec<super::LlmProfileRow> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at FROM llm_profiles ORDER BY created_at DESC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(super::LlmProfileRow {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                model: row.get(3)?,
                base_url: row.get(4)?,
                vault_key_id: row.get(5)?,
                max_tokens: row.get::<_, Option<i32>>(6)?.map(|v| v as u32),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_llm_profile(&self, id: &str) -> Option<super::LlmProfileRow> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at FROM llm_profiles WHERE id = ?1",
            params![id],
            |row| {
                Ok(super::LlmProfileRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    model: row.get(3)?,
                    base_url: row.get(4)?,
                    vault_key_id: row.get(5)?,
                    max_tokens: row.get::<_, Option<i32>>(6)?.map(|v| v as u32),
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .ok()
    }

    fn upsert_llm_profile(&self, row: &super::LlmProfileRow) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO llm_profiles (id, name, kind, model, base_url, vault_key_id, max_tokens, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET name = ?2, kind = ?3, model = ?4, base_url = ?5, vault_key_id = ?6, max_tokens = ?7, updated_at = ?9",
            params![
                row.id,
                row.name,
                row.kind,
                row.model,
                row.base_url,
                row.vault_key_id,
                row.max_tokens.map(|v| v as i32),
                row.created_at,
                row.updated_at,
            ],
        )
        .ok();
    }

    fn delete_llm_profile(&self, id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM llm_profiles WHERE id = ?1", params![id])
            .ok();
    }

    // ── Inference Logs ────────────────────────────────────────────────

    fn insert_inference_log(&self, entry: &super::InferenceLogEntry) {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "INSERT INTO inference_logs (
                id, timestamp, user_id, workspace_id, org_id, dept_id, session_id,
                response_id, query_text, detected_language, intent, complexity,
                llm_kind, llm_model, settings_scope,
                prompt_tokens, completion_tokens, total_ms, search_ms, generation_ms,
                chunks_retrieved, avg_chunk_score, self_rag_decision, self_rag_confidence,
                quality_guard_pass, relevance_score, hallucination_score, completeness_score,
                pipeline_route, agents_used, status, error_message, response_length,
                feedback_score
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
                ?29, ?30, ?31, ?32, ?33, ?34
            )",
            params![
                entry.id,
                entry.timestamp,
                entry.user_id,
                entry.workspace_id,
                entry.org_id,
                entry.dept_id,
                entry.session_id,
                entry.response_id,
                entry.query_text,
                entry.detected_language,
                entry.intent,
                entry.complexity,
                entry.llm_kind,
                entry.llm_model,
                entry.settings_scope,
                entry.prompt_tokens,
                entry.completion_tokens,
                entry.total_ms as i64,
                entry.search_ms.map(|v| v as i64),
                entry.generation_ms.map(|v| v as i64),
                entry.chunks_retrieved.map(|v| v as i32),
                entry.avg_chunk_score,
                entry.self_rag_decision,
                entry.self_rag_confidence,
                entry.quality_guard_pass.map(|v| v as i32),
                entry.relevance_score,
                entry.hallucination_score,
                entry.completeness_score,
                entry.pipeline_route,
                entry.agents_used,
                entry.status,
                entry.error_message,
                entry.response_length as i32,
                entry.feedback_score.map(|v| v as i32),
            ],
        )
        .ok();

        // Log retention: if count exceeds 50000, delete oldest 10%
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM inference_logs", [], |row| row.get(0))
            .unwrap_or(0);
        if count > 50_000 {
            let to_delete = count / 10;
            tx.execute(
                "DELETE FROM inference_logs WHERE id IN (
                    SELECT id FROM inference_logs ORDER BY timestamp ASC LIMIT ?1
                )",
                params![to_delete],
            )
            .ok();
        }
        tx.commit().ok();
    }

    fn list_inference_logs(
        &self,
        filter: &super::InferenceLogFilter,
    ) -> Vec<super::InferenceLogEntry> {
        let conn = self.conn.lock().unwrap();

        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            param_values.push(Box::new(ws.clone()));
            conditions.push(format!("workspace_id = ?{}", param_values.len()));
        }
        if let Some(ref uid) = filter.user_id {
            param_values.push(Box::new(uid.clone()));
            conditions.push(format!("user_id = ?{}", param_values.len()));
        }
        if let Some(ref from) = filter.from_timestamp {
            param_values.push(Box::new(from.clone()));
            conditions.push(format!("timestamp >= ?{}", param_values.len()));
        }
        if let Some(ref to) = filter.to_timestamp {
            param_values.push(Box::new(to.clone()));
            conditions.push(format!("timestamp <= ?{}", param_values.len()));
        }
        if let Some(ref status) = filter.status {
            param_values.push(Box::new(status.clone()));
            conditions.push(format!("status = ?{}", param_values.len()));
        }
        if let Some(ref model) = filter.llm_model {
            param_values.push(Box::new(model.clone()));
            conditions.push(format!("llm_model = ?{}", param_values.len()));
        }
        if let Some(ref intent) = filter.intent {
            param_values.push(Box::new(intent.clone()));
            conditions.push(format!("intent = ?{}", param_values.len()));
        }
        if let Some(ref response_id) = filter.response_id {
            param_values.push(Box::new(response_id.clone()));
            conditions.push(format!("response_id = ?{}", param_values.len()));
        }
        if let Some(ref session_id) = filter.session_id {
            param_values.push(Box::new(session_id.clone()));
            conditions.push(format!("session_id = ?{}", param_values.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let limit = if filter.limit == 0 { 100 } else { filter.limit };
        param_values.push(Box::new(limit as i64));
        let limit_idx = param_values.len();
        param_values.push(Box::new(filter.offset as i64));
        let offset_idx = param_values.len();

        let sql = format!(
            "SELECT id, timestamp, user_id, workspace_id, org_id, dept_id, session_id,
                    response_id, query_text, detected_language, intent, complexity,
                    llm_kind, llm_model, settings_scope,
                    prompt_tokens, completion_tokens, total_ms, search_ms, generation_ms,
                    chunks_retrieved, avg_chunk_score, self_rag_decision, self_rag_confidence,
                    quality_guard_pass, relevance_score, hallucination_score, completeness_score,
                    pipeline_route, agents_used, status, error_message, response_length,
                    feedback_score
             FROM inference_logs {where_clause}
             ORDER BY timestamp DESC
             LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
        );

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map(params_refs.as_slice(), |row| {
            Ok(super::InferenceLogEntry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                user_id: row.get(2)?,
                workspace_id: row.get(3)?,
                org_id: row.get(4)?,
                dept_id: row.get(5)?,
                session_id: row.get(6)?,
                response_id: row.get(7)?,
                query_text: row.get(8)?,
                detected_language: row.get(9)?,
                intent: row.get(10)?,
                complexity: row.get(11)?,
                llm_kind: row.get(12)?,
                llm_model: row.get(13)?,
                settings_scope: row.get(14)?,
                prompt_tokens: row.get::<_, i32>(15)? as u32,
                completion_tokens: row.get::<_, i32>(16)? as u32,
                total_ms: row.get::<_, i64>(17)? as u64,
                search_ms: row.get::<_, Option<i64>>(18)?.map(|v| v as u64),
                generation_ms: row.get::<_, Option<i64>>(19)?.map(|v| v as u64),
                chunks_retrieved: row.get::<_, Option<i32>>(20)?.map(|v| v as u32),
                avg_chunk_score: row.get(21)?,
                self_rag_decision: row.get(22)?,
                self_rag_confidence: row.get(23)?,
                quality_guard_pass: row.get::<_, Option<i32>>(24)?.map(|v| v != 0),
                relevance_score: row.get(25)?,
                hallucination_score: row.get(26)?,
                completeness_score: row.get(27)?,
                pipeline_route: row.get(28)?,
                agents_used: row.get(29)?,
                status: row.get(30)?,
                error_message: row.get(31)?,
                response_length: row.get::<_, i32>(32)? as u32,
                feedback_score: row.get::<_, Option<i32>>(33)?.map(|v| v as i8),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_inference_stats(&self, filter: &super::InferenceLogFilter) -> super::InferenceStats {
        let conn = self.conn.lock().unwrap();

        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            param_values.push(Box::new(ws.clone()));
            conditions.push(format!("workspace_id = ?{}", param_values.len()));
        }
        if let Some(ref uid) = filter.user_id {
            param_values.push(Box::new(uid.clone()));
            conditions.push(format!("user_id = ?{}", param_values.len()));
        }
        if let Some(ref from) = filter.from_timestamp {
            param_values.push(Box::new(from.clone()));
            conditions.push(format!("timestamp >= ?{}", param_values.len()));
        }
        if let Some(ref to) = filter.to_timestamp {
            param_values.push(Box::new(to.clone()));
            conditions.push(format!("timestamp <= ?{}", param_values.len()));
        }
        if let Some(ref status) = filter.status {
            param_values.push(Box::new(status.clone()));
            conditions.push(format!("status = ?{}", param_values.len()));
        }
        if let Some(ref model) = filter.llm_model {
            param_values.push(Box::new(model.clone()));
            conditions.push(format!("llm_model = ?{}", param_values.len()));
        }
        if let Some(ref intent) = filter.intent {
            param_values.push(Box::new(intent.clone()));
            conditions.push(format!("intent = ?{}", param_values.len()));
        }
        if let Some(ref response_id) = filter.response_id {
            param_values.push(Box::new(response_id.clone()));
            conditions.push(format!("response_id = ?{}", param_values.len()));
        }
        if let Some(ref session_id) = filter.session_id {
            param_values.push(Box::new(session_id.clone()));
            conditions.push(format!("session_id = ?{}", param_values.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        // Aggregate stats
        let sql = format!(
            "SELECT
                COUNT(*),
                COALESCE(AVG(total_ms), 0),
                COALESCE(AVG(search_ms), 0),
                COALESCE(AVG(generation_ms), 0),
                COALESCE(AVG(relevance_score), 0),
                COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0),
                COALESCE(SUM(CASE WHEN status = 'success' THEN 1.0 ELSE 0.0 END) / NULLIF(COUNT(*), 0), 0),
                COALESCE(SUM(CASE WHEN quality_guard_pass = 1 THEN 1.0 ELSE 0.0 END) / NULLIF(SUM(CASE WHEN quality_guard_pass IS NOT NULL THEN 1 ELSE 0 END), 0), 0),
                COALESCE(SUM(CASE WHEN feedback_score > 0 THEN 1.0 ELSE 0.0 END) / NULLIF(SUM(CASE WHEN feedback_score IS NOT NULL THEN 1 ELSE 0 END), 0), 0)
             FROM inference_logs {where_clause}"
        );

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
        ) = conn
            .query_row(&sql, params_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                    row.get::<_, f64>(7)?,
                    row.get::<_, f64>(8)?,
                    row.get::<_, f64>(9)?,
                ))
            })
            .unwrap_or((0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0.0, 0.0, 0.0));

        // By model
        let model_sql = format!(
            "SELECT llm_model, COUNT(*), COALESCE(AVG(total_ms), 0),
                    COALESCE(AVG(relevance_score), 0),
                    COALESCE(SUM(prompt_tokens) + SUM(completion_tokens), 0)
             FROM inference_logs {where_clause}
             GROUP BY llm_model ORDER BY COUNT(*) DESC"
        );
        let by_model = {
            let mut stmt = conn.prepare(&model_sql).unwrap();
            stmt.query_map(params_refs.as_slice(), |row| {
                Ok(super::ModelStats {
                    model: row.get(0)?,
                    count: row.get::<_, i64>(1)? as u64,
                    avg_ms: row.get(2)?,
                    avg_quality: row.get(3)?,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

        // By workspace
        let ws_sql = format!(
            "SELECT COALESCE(workspace_id, ''), COUNT(*), COALESCE(AVG(total_ms), 0),
                    COALESCE(SUM(prompt_tokens) + SUM(completion_tokens), 0)
             FROM inference_logs {where_clause}
             GROUP BY workspace_id ORDER BY COUNT(*) DESC"
        );
        let by_workspace = {
            let mut stmt = conn.prepare(&ws_sql).unwrap();
            stmt.query_map(params_refs.as_slice(), |row| {
                Ok(super::WorkspaceStats {
                    workspace_id: row.get(0)?,
                    count: row.get::<_, i64>(1)? as u64,
                    avg_ms: row.get(2)?,
                    total_tokens: row.get::<_, i64>(3)? as u64,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE inference_logs SET feedback_score = ?1 WHERE response_id = ?2",
            params![score as i32, response_id],
        )
        .ok();
    }

    fn delete_inference_logs(&self, filter: &super::InferenceLogFilter) -> u64 {
        let conn = self.conn.lock().unwrap();

        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            param_values.push(Box::new(ws.clone()));
            conditions.push(format!("workspace_id = ?{}", param_values.len()));
        }
        if let Some(ref uid) = filter.user_id {
            param_values.push(Box::new(uid.clone()));
            conditions.push(format!("user_id = ?{}", param_values.len()));
        }
        if let Some(ref from) = filter.from_timestamp {
            param_values.push(Box::new(from.clone()));
            conditions.push(format!("timestamp >= ?{}", param_values.len()));
        }
        if let Some(ref to) = filter.to_timestamp {
            param_values.push(Box::new(to.clone()));
            conditions.push(format!("timestamp <= ?{}", param_values.len()));
        }
        if let Some(ref status) = filter.status {
            param_values.push(Box::new(status.clone()));
            conditions.push(format!("status = ?{}", param_values.len()));
        }
        if let Some(ref model) = filter.llm_model {
            param_values.push(Box::new(model.clone()));
            conditions.push(format!("llm_model = ?{}", param_values.len()));
        }
        if let Some(ref intent) = filter.intent {
            param_values.push(Box::new(intent.clone()));
            conditions.push(format!("intent = ?{}", param_values.len()));
        }
        if let Some(ref response_id) = filter.response_id {
            param_values.push(Box::new(response_id.clone()));
            conditions.push(format!("response_id = ?{}", param_values.len()));
        }
        if let Some(ref session_id) = filter.session_id {
            param_values.push(Box::new(session_id.clone()));
            conditions.push(format!("session_id = ?{}", param_values.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!("DELETE FROM inference_logs {where_clause}");
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        conn.execute(&sql, params_refs.as_slice()).unwrap_or(0) as u64
    }

    fn count_inference_logs(&self, filter: &super::InferenceLogFilter) -> u64 {
        let conn = self.conn.lock().unwrap();

        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref ws) = filter.workspace_id {
            param_values.push(Box::new(ws.clone()));
            conditions.push(format!("workspace_id = ?{}", param_values.len()));
        }
        if let Some(ref uid) = filter.user_id {
            param_values.push(Box::new(uid.clone()));
            conditions.push(format!("user_id = ?{}", param_values.len()));
        }
        if let Some(ref from) = filter.from_timestamp {
            param_values.push(Box::new(from.clone()));
            conditions.push(format!("timestamp >= ?{}", param_values.len()));
        }
        if let Some(ref to) = filter.to_timestamp {
            param_values.push(Box::new(to.clone()));
            conditions.push(format!("timestamp <= ?{}", param_values.len()));
        }
        if let Some(ref status) = filter.status {
            param_values.push(Box::new(status.clone()));
            conditions.push(format!("status = ?{}", param_values.len()));
        }
        if let Some(ref model) = filter.llm_model {
            param_values.push(Box::new(model.clone()));
            conditions.push(format!("llm_model = ?{}", param_values.len()));
        }
        if let Some(ref intent) = filter.intent {
            param_values.push(Box::new(intent.clone()));
            conditions.push(format!("intent = ?{}", param_values.len()));
        }
        if let Some(ref response_id) = filter.response_id {
            param_values.push(Box::new(response_id.clone()));
            conditions.push(format!("response_id = ?{}", param_values.len()));
        }
        if let Some(ref session_id) = filter.session_id {
            param_values.push(Box::new(session_id.clone()));
            conditions.push(format!("session_id = ?{}", param_values.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!("SELECT COUNT(*) FROM inference_logs {where_clause}");
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        conn.query_row(&sql, params_refs.as_slice(), |row| row.get::<_, i64>(0))
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
        let conn = self.conn.lock().unwrap();
        let id = ApiKeyId::new();
        let now = ts(&chrono::Utc::now());
        conn.execute(
            "INSERT INTO api_keys (id, name, key_hash, key_prefix, user_id, role, created_at, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            params![
                id.0.to_string(),
                name,
                key_hash,
                key_prefix,
                user_id.0.to_string(),
                role,
                now,
            ],
        )
        .map_err(|e| ThaiRagError::Database(format!("Failed to create API key: {e}")))?;

        Ok(super::ApiKeyRow {
            id,
            name,
            key_hash,
            key_prefix,
            user_id,
            role,
            created_at: now,
            last_used_at: None,
            is_active: true,
        })
    }

    fn get_api_key_by_hash(&self, key_hash: &str) -> Option<super::ApiKeyRow> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, key_hash, key_prefix, user_id, role, created_at, last_used_at, is_active
             FROM api_keys WHERE key_hash = ?1",
            params![key_hash],
            |row| {
                Ok(super::ApiKeyRow {
                    id: ApiKeyId(parse_uuid(&row.get::<_, String>(0)?)),
                    name: row.get(1)?,
                    key_hash: row.get(2)?,
                    key_prefix: row.get(3)?,
                    user_id: UserId(parse_uuid(&row.get::<_, String>(4)?)),
                    role: row.get(5)?,
                    created_at: row.get(6)?,
                    last_used_at: row.get(7)?,
                    is_active: row.get::<_, i32>(8)? != 0,
                })
            },
        )
        .ok()
    }

    fn list_api_keys(&self, user_id: UserId) -> Vec<super::ApiKeyRow> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, key_hash, key_prefix, user_id, role, created_at, last_used_at, is_active
                 FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC",
            )
            .unwrap();
        stmt.query_map(params![user_id.0.to_string()], |row| {
            Ok(super::ApiKeyRow {
                id: ApiKeyId(parse_uuid(&row.get::<_, String>(0)?)),
                name: row.get(1)?,
                key_hash: row.get(2)?,
                key_prefix: row.get(3)?,
                user_id: UserId(parse_uuid(&row.get::<_, String>(4)?)),
                role: row.get(5)?,
                created_at: row.get(6)?,
                last_used_at: row.get(7)?,
                is_active: row.get::<_, i32>(8)? != 0,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn revoke_api_key(&self, key_id: ApiKeyId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE api_keys SET is_active = 0 WHERE id = ?1",
                params![key_id.0.to_string()],
            )
            .map_err(|e| ThaiRagError::Database(format!("Failed to revoke API key: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "API key {key_id} not found"
            )));
        }
        Ok(())
    }

    fn touch_api_key(&self, key_id: ApiKeyId) {
        let conn = self.conn.lock().unwrap();
        let now = ts(&chrono::Utc::now());
        let _ = conn.execute(
            "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
            params![now, key_id.0.to_string()],
        );
    }

    // ── Knowledge Graph ──────────────────────────────────────────────

    fn upsert_entity(
        &self,
        name: &str,
        entity_type: &str,
        workspace_id: WorkspaceId,
        metadata: serde_json::Value,
    ) -> Result<thairag_core::types::Entity> {
        let conn = self.conn.lock().unwrap();
        let ws_str = workspace_id.0.to_string();
        // Try to find existing entity by name+type+workspace
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM entities WHERE name = ?1 AND entity_type = ?2 AND workspace_id = ?3",
                params![name, entity_type, ws_str],
                |row| row.get(0),
            )
            .ok();

        if let Some(id_str) = existing {
            // Update metadata
            let meta_str = metadata.to_string();
            conn.execute(
                "UPDATE entities SET metadata = ?1 WHERE id = ?2",
                params![meta_str, id_str],
            )
            .ok();
            let id = thairag_core::types::EntityId(parse_uuid(&id_str));
            // Fetch doc_ids
            let mut stmt = conn
                .prepare("SELECT doc_id FROM entity_doc_links WHERE entity_id = ?1")
                .unwrap();
            let doc_ids: Vec<DocId> = stmt
                .query_map(params![id_str], |row| {
                    Ok(DocId(parse_uuid(&row.get::<_, String>(0)?)))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            let created_at: String = conn
                .query_row(
                    "SELECT created_at FROM entities WHERE id = ?1",
                    params![id_str],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            return Ok(thairag_core::types::Entity {
                id,
                name: name.to_string(),
                entity_type: entity_type.to_string(),
                workspace_id,
                doc_ids,
                metadata,
                created_at,
            });
        }

        let id = thairag_core::types::EntityId::new();
        let now = ts(&chrono::Utc::now());
        let meta_str = metadata.to_string();
        conn.execute(
            "INSERT INTO entities (id, name, entity_type, workspace_id, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id.0.to_string(), name, entity_type, ws_str, meta_str, now],
        )
        .map_err(|e| ThaiRagError::Database(format!("Failed to insert entity: {e}")))?;

        Ok(thairag_core::types::Entity {
            id,
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            workspace_id,
            doc_ids: vec![],
            metadata,
            created_at: now,
        })
    }

    fn add_entity_doc_link(
        &self,
        entity_id: thairag_core::types::EntityId,
        doc_id: DocId,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO entity_doc_links (entity_id, doc_id) VALUES (?1, ?2)",
            params![entity_id.0.to_string(), doc_id.0.to_string()],
        )
        .map_err(|e| ThaiRagError::Database(format!("Failed to add entity doc link: {e}")))?;
        Ok(())
    }

    fn insert_relation(
        &self,
        from_id: thairag_core::types::EntityId,
        to_id: thairag_core::types::EntityId,
        relation_type: &str,
        confidence: f32,
        doc_id: DocId,
    ) -> Result<thairag_core::types::Relation> {
        let conn = self.conn.lock().unwrap();
        let id = thairag_core::types::RelationId::new();
        let now = ts(&chrono::Utc::now());
        conn.execute(
            "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, confidence, doc_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id.0.to_string(),
                from_id.0.to_string(),
                to_id.0.to_string(),
                relation_type,
                confidence,
                doc_id.0.to_string(),
                now,
            ],
        )
        .map_err(|e| ThaiRagError::Database(format!("Failed to insert relation: {e}")))?;

        Ok(thairag_core::types::Relation {
            id,
            from_entity_id: from_id,
            to_entity_id: to_id,
            relation_type: relation_type.to_string(),
            confidence,
            doc_id,
            created_at: now,
        })
    }

    fn list_entities(&self, workspace_id: WorkspaceId) -> Vec<thairag_core::types::Entity> {
        let conn = self.conn.lock().unwrap();
        let ws_str = workspace_id.0.to_string();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, entity_type, metadata, created_at FROM entities WHERE workspace_id = ?1 ORDER BY name",
            )
            .unwrap();
        let entities: Vec<_> = stmt
            .query_map(params![ws_str], |row| {
                let id_str: String = row.get(0)?;
                let meta_str: String = row.get(3)?;
                Ok((
                    id_str,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    meta_str,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        entities
            .into_iter()
            .map(|(id_str, name, entity_type, meta_str, created_at)| {
                let id = thairag_core::types::EntityId(parse_uuid(&id_str));
                let mut doc_stmt = conn
                    .prepare("SELECT doc_id FROM entity_doc_links WHERE entity_id = ?1")
                    .unwrap();
                let doc_ids: Vec<DocId> = doc_stmt
                    .query_map(params![id_str], |row| {
                        Ok(DocId(parse_uuid(&row.get::<_, String>(0)?)))
                    })
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                thairag_core::types::Entity {
                    id,
                    name,
                    entity_type,
                    workspace_id,
                    doc_ids,
                    metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
                    created_at,
                }
            })
            .collect()
    }

    fn get_entity_relations(
        &self,
        entity_id: thairag_core::types::EntityId,
    ) -> Vec<thairag_core::types::Relation> {
        let conn = self.conn.lock().unwrap();
        let id_str = entity_id.0.to_string();
        let mut stmt = conn
            .prepare(
                "SELECT id, from_entity_id, to_entity_id, relation_type, confidence, doc_id, created_at
                 FROM relations WHERE from_entity_id = ?1 OR to_entity_id = ?1",
            )
            .unwrap();
        stmt.query_map(params![id_str], |row| {
            Ok(thairag_core::types::Relation {
                id: thairag_core::types::RelationId(parse_uuid(&row.get::<_, String>(0)?)),
                from_entity_id: thairag_core::types::EntityId(parse_uuid(
                    &row.get::<_, String>(1)?,
                )),
                to_entity_id: thairag_core::types::EntityId(parse_uuid(&row.get::<_, String>(2)?)),
                relation_type: row.get(3)?,
                confidence: row.get(4)?,
                doc_id: DocId(parse_uuid(&row.get::<_, String>(5)?)),
                created_at: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn search_entities(
        &self,
        workspace_id: WorkspaceId,
        query: &str,
    ) -> Vec<thairag_core::types::Entity> {
        let conn = self.conn.lock().unwrap();
        let ws_str = workspace_id.0.to_string();
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT id, name, entity_type, metadata, created_at FROM entities
                 WHERE workspace_id = ?1 AND name LIKE ?2 ORDER BY name LIMIT 100",
            )
            .unwrap();
        let entities: Vec<_> = stmt
            .query_map(params![ws_str, pattern], |row| {
                let id_str: String = row.get(0)?;
                let meta_str: String = row.get(3)?;
                Ok((
                    id_str,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    meta_str,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        entities
            .into_iter()
            .map(|(id_str, name, entity_type, meta_str, created_at)| {
                let id = thairag_core::types::EntityId(parse_uuid(&id_str));
                let mut doc_stmt = conn
                    .prepare("SELECT doc_id FROM entity_doc_links WHERE entity_id = ?1")
                    .unwrap();
                let doc_ids: Vec<DocId> = doc_stmt
                    .query_map(params![id_str], |row| {
                        Ok(DocId(parse_uuid(&row.get::<_, String>(0)?)))
                    })
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                thairag_core::types::Entity {
                    id,
                    name,
                    entity_type,
                    workspace_id,
                    doc_ids,
                    metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
                    created_at,
                }
            })
            .collect()
    }

    fn get_knowledge_graph(
        &self,
        workspace_id: WorkspaceId,
    ) -> thairag_core::types::KnowledgeGraph {
        let entities = self.list_entities(workspace_id);
        let conn = self.conn.lock().unwrap();
        let ws_str = workspace_id.0.to_string();
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.from_entity_id, r.to_entity_id, r.relation_type, r.confidence, r.doc_id, r.created_at
                 FROM relations r
                 JOIN entities e ON r.from_entity_id = e.id
                 WHERE e.workspace_id = ?1",
            )
            .unwrap();
        let relations: Vec<thairag_core::types::Relation> = stmt
            .query_map(params![ws_str], |row| {
                Ok(thairag_core::types::Relation {
                    id: thairag_core::types::RelationId(parse_uuid(&row.get::<_, String>(0)?)),
                    from_entity_id: thairag_core::types::EntityId(parse_uuid(
                        &row.get::<_, String>(1)?,
                    )),
                    to_entity_id: thairag_core::types::EntityId(parse_uuid(
                        &row.get::<_, String>(2)?,
                    )),
                    relation_type: row.get(3)?,
                    confidence: row.get(4)?,
                    doc_id: DocId(parse_uuid(&row.get::<_, String>(5)?)),
                    created_at: row.get(6)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        thairag_core::types::KnowledgeGraph {
            entities,
            relations,
        }
    }

    fn delete_entity(&self, entity_id: thairag_core::types::EntityId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let id_str = entity_id.0.to_string();
        // Relations and doc links are cascade-deleted by foreign keys
        let affected = conn
            .execute("DELETE FROM entities WHERE id = ?1", params![id_str])
            .map_err(|e| ThaiRagError::Database(format!("Failed to delete entity: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Entity {entity_id} not found"
            )));
        }
        Ok(())
    }

    fn get_entity(
        &self,
        entity_id: thairag_core::types::EntityId,
    ) -> Result<thairag_core::types::Entity> {
        let conn = self.conn.lock().unwrap();
        let id_str = entity_id.0.to_string();
        let (name, entity_type, ws_str, meta_str, created_at): (String, String, String, String, String) = conn
            .query_row(
                "SELECT name, entity_type, workspace_id, metadata, created_at FROM entities WHERE id = ?1",
                params![id_str],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .map_err(|_| ThaiRagError::NotFound(format!("Entity {entity_id} not found")))?;

        let mut doc_stmt = conn
            .prepare("SELECT doc_id FROM entity_doc_links WHERE entity_id = ?1")
            .unwrap();
        let doc_ids: Vec<DocId> = doc_stmt
            .query_map(params![id_str], |row| {
                Ok(DocId(parse_uuid(&row.get::<_, String>(0)?)))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        Ok(thairag_core::types::Entity {
            id: entity_id,
            name,
            entity_type,
            workspace_id: WorkspaceId(parse_uuid(&ws_str)),
            doc_ids,
            metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
            created_at,
        })
    }

    // ── Workspace ACLs ──────────────────────────────────────────────

    fn grant_workspace_access(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        permission: AclPermission,
        granted_by: Option<UserId>,
    ) -> Result<WorkspaceAcl> {
        let conn = self.conn.lock().unwrap();
        let now = ts(&Utc::now());
        let uid = user_id.0.to_string();
        let wid = workspace_id.0.to_string();
        let perm_str = permission.as_str();
        let gb = granted_by.map(|u| u.0.to_string());
        conn.execute(
            "INSERT INTO workspace_acls (user_id, workspace_id, permission, granted_at, granted_by)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(user_id, workspace_id) DO UPDATE SET
                permission = excluded.permission,
                granted_at = excluded.granted_at,
                granted_by = excluded.granted_by",
            params![uid, wid, perm_str, now, gb],
        )
        .map_err(|e| ThaiRagError::Database(format!("Failed to grant workspace access: {e}")))?;
        Ok(WorkspaceAcl {
            user_id,
            workspace_id,
            permission,
            granted_at: now,
            granted_by,
        })
    }

    fn revoke_workspace_access(&self, user_id: UserId, workspace_id: WorkspaceId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM workspace_acls WHERE user_id = ?1 AND workspace_id = ?2",
                params![user_id.0.to_string(), workspace_id.0.to_string()],
            )
            .map_err(|e| {
                ThaiRagError::Database(format!("Failed to revoke workspace access: {e}"))
            })?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(
                "Workspace ACL entry not found".into(),
            ));
        }
        Ok(())
    }

    fn list_workspace_acls(&self, workspace_id: WorkspaceId) -> Vec<WorkspaceAcl> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT user_id, workspace_id, permission, granted_at, granted_by
                 FROM workspace_acls WHERE workspace_id = ?1 ORDER BY granted_at",
            )
            .unwrap();
        stmt.query_map(params![workspace_id.0.to_string()], |row| {
            Ok(WorkspaceAcl {
                user_id: UserId(parse_uuid(&row.get::<_, String>(0)?)),
                workspace_id: WorkspaceId(parse_uuid(&row.get::<_, String>(1)?)),
                permission: AclPermission::from_str_lossy(&row.get::<_, String>(2)?),
                granted_at: row.get(3)?,
                granted_by: row
                    .get::<_, Option<String>>(4)?
                    .map(|s| UserId(parse_uuid(&s))),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_user_workspace_acl(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
    ) -> Option<AclPermission> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT permission FROM workspace_acls WHERE user_id = ?1 AND workspace_id = ?2",
            params![user_id.0.to_string(), workspace_id.0.to_string()],
            |row| Ok(AclPermission::from_str_lossy(&row.get::<_, String>(0)?)),
        )
        .ok()
    }

    fn list_accessible_workspaces(&self, user_id: UserId) -> Vec<WorkspaceId> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT workspace_id FROM workspace_acls WHERE user_id = ?1")
            .unwrap();
        stmt.query_map(params![user_id.0.to_string()], |row| {
            Ok(WorkspaceId(parse_uuid(&row.get::<_, String>(0)?)))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── Document ACLs ───────────────────────────────────────────────

    fn grant_document_access(
        &self,
        user_id: UserId,
        doc_id: DocId,
        permission: AclPermission,
    ) -> Result<DocumentAcl> {
        let conn = self.conn.lock().unwrap();
        let now = ts(&Utc::now());
        let uid = user_id.0.to_string();
        let did = doc_id.0.to_string();
        let perm_str = permission.as_str();
        conn.execute(
            "INSERT INTO document_acls (user_id, doc_id, permission, granted_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id, doc_id) DO UPDATE SET
                permission = excluded.permission,
                granted_at = excluded.granted_at",
            params![uid, did, perm_str, now],
        )
        .map_err(|e| ThaiRagError::Database(format!("Failed to grant document access: {e}")))?;
        Ok(DocumentAcl {
            user_id,
            doc_id,
            permission,
            granted_at: now,
        })
    }

    fn revoke_document_access(&self, user_id: UserId, doc_id: DocId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "DELETE FROM document_acls WHERE user_id = ?1 AND doc_id = ?2",
                params![user_id.0.to_string(), doc_id.0.to_string()],
            )
            .map_err(|e| {
                ThaiRagError::Database(format!("Failed to revoke document access: {e}"))
            })?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(
                "Document ACL entry not found".into(),
            ));
        }
        Ok(())
    }

    fn check_document_access(&self, user_id: UserId, doc_id: DocId) -> Option<AclPermission> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT permission FROM document_acls WHERE user_id = ?1 AND doc_id = ?2",
            params![user_id.0.to_string(), doc_id.0.to_string()],
            |row| Ok(AclPermission::from_str_lossy(&row.get::<_, String>(0)?)),
        )
        .ok()
    }

    // ── Search Analytics ────────────────────────────────────────────────

    fn insert_search_event(&self, event: &super::SearchAnalyticsEvent) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO search_analytics_events
             (id, timestamp, query_text, user_id, workspace_id, result_count, latency_ms, zero_results)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.id,
                event.timestamp,
                event.query_text,
                event.user_id,
                event.workspace_id,
                event.result_count as i64,
                event.latency_ms as i64,
                event.zero_results as i32,
            ],
        )
        .ok();
    }

    fn list_search_events(
        &self,
        filter: &super::SearchAnalyticsFilter,
    ) -> Vec<super::SearchAnalyticsEvent> {
        let conn = self.conn.lock().unwrap();
        let mut conditions = Vec::<String>::new();
        if let Some(from) = &filter.from {
            conditions.push(format!("timestamp >= '{from}'"));
        }
        if let Some(to) = &filter.to {
            conditions.push(format!("timestamp <= '{to}'"));
        }
        if let Some(ws) = &filter.workspace_id {
            conditions.push(format!("workspace_id = '{ws}'"));
        }
        if let Some(uid) = &filter.user_id {
            conditions.push(format!("user_id = '{uid}'"));
        }
        if filter.zero_results_only {
            conditions.push("zero_results = 1".to_string());
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let limit = filter.limit.unwrap_or(1000);
        let offset = filter.offset.unwrap_or(0);
        let sql = format!(
            "SELECT id, timestamp, query_text, user_id, workspace_id, result_count, latency_ms, zero_results
             FROM search_analytics_events {where_clause}
             ORDER BY timestamp DESC
             LIMIT {limit} OFFSET {offset}"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |row| {
            Ok(super::SearchAnalyticsEvent {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                query_text: row.get(2)?,
                user_id: row.get(3)?,
                workspace_id: row.get(4)?,
                result_count: row.get::<_, i64>(5)? as u32,
                latency_ms: row.get::<_, i64>(6)? as u64,
                zero_results: row.get::<_, i32>(7)? != 0,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_popular_queries(&self, limit: usize) -> Vec<super::PopularQuery> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT query_text, COUNT(*) as cnt, AVG(CAST(result_count AS REAL)) as avg_results,
             AVG(CAST(latency_ms AS REAL)) as avg_latency
             FROM search_analytics_events
             GROUP BY query_text
             ORDER BY cnt DESC
             LIMIT {limit}"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |row| {
            Ok(super::PopularQuery {
                query_text: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
                avg_results: row.get(2)?,
                avg_latency_ms: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_search_analytics_summary(
        &self,
        filter: &super::SearchAnalyticsFilter,
    ) -> super::SearchAnalyticsSummary {
        let conn = self.conn.lock().unwrap();
        let mut conditions = Vec::<String>::new();
        if let Some(from) = &filter.from {
            conditions.push(format!("timestamp >= '{from}'"));
        }
        if let Some(to) = &filter.to {
            conditions.push(format!("timestamp <= '{to}'"));
        }
        if let Some(ws) = &filter.workspace_id {
            conditions.push(format!("workspace_id = '{ws}'"));
        }
        if let Some(uid) = &filter.user_id {
            conditions.push(format!("user_id = '{uid}'"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let (total_searches, zero_result_count, avg_latency_ms, avg_results): (u64, u64, f64, f64) =
            conn.query_row(
                &format!(
                    "SELECT COUNT(*), SUM(CASE WHEN zero_results = 1 THEN 1 ELSE 0 END),
                     AVG(CAST(latency_ms AS REAL)), AVG(CAST(result_count AS REAL))
                     FROM search_analytics_events {where_clause}"
                ),
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0).unwrap_or(0) as u64,
                        row.get::<_, i64>(1).unwrap_or(0) as u64,
                        row.get::<_, f64>(2).unwrap_or(0.0),
                        row.get::<_, f64>(3).unwrap_or(0.0),
                    ))
                },
            )
            .unwrap_or((0, 0, 0.0, 0.0));

        let per_day_sql = format!(
            "SELECT substr(timestamp, 1, 10) as day, COUNT(*) as cnt
             FROM search_analytics_events {where_clause}
             GROUP BY day ORDER BY day"
        );
        let searches_per_day: Vec<(String, u64)> =
            if let Ok(mut per_day_stmt) = conn.prepare(&per_day_sql) {
                per_day_stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
                    })
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect()
            } else {
                vec![]
            };

        super::SearchAnalyticsSummary {
            total_searches,
            zero_result_count,
            avg_latency_ms,
            avg_results,
            searches_per_day,
        }
    }

    // ── Document Lineage ────────────────────────────────────────────────

    fn insert_lineage_record(&self, record: &super::LineageRecord) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO lineage_records
             (id, response_id, timestamp, query_text, chunk_id, doc_id, doc_title,
              chunk_text_preview, score, rank, contributed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.id,
                record.response_id,
                record.timestamp,
                record.query_text,
                record.chunk_id,
                record.doc_id,
                record.doc_title,
                record.chunk_text_preview,
                record.score as f64,
                record.rank as i64,
                record.contributed as i32,
            ],
        )
        .ok();
    }

    fn get_lineage_for_response(&self, response_id: &str) -> Vec<super::LineageRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, response_id, timestamp, query_text, chunk_id, doc_id, doc_title,
             chunk_text_preview, score, rank, contributed
             FROM lineage_records WHERE response_id = ?1 ORDER BY rank ASC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![response_id], |row| {
            Ok(super::LineageRecord {
                id: row.get(0)?,
                response_id: row.get(1)?,
                timestamp: row.get(2)?,
                query_text: row.get(3)?,
                chunk_id: row.get(4)?,
                doc_id: row.get(5)?,
                doc_title: row.get(6)?,
                chunk_text_preview: row.get(7)?,
                score: row.get::<_, f64>(8)? as f32,
                rank: row.get::<_, i64>(9)? as u32,
                contributed: row.get::<_, i32>(10)? != 0,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_lineage_for_document(&self, doc_id: &str, limit: usize) -> Vec<super::LineageRecord> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT id, response_id, timestamp, query_text, chunk_id, doc_id, doc_title,
             chunk_text_preview, score, rank, contributed
             FROM lineage_records WHERE doc_id = ?1 ORDER BY timestamp DESC LIMIT {limit}"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![doc_id], |row| {
            Ok(super::LineageRecord {
                id: row.get(0)?,
                response_id: row.get(1)?,
                timestamp: row.get(2)?,
                query_text: row.get(3)?,
                chunk_id: row.get(4)?,
                doc_id: row.get(5)?,
                doc_title: row.get(6)?,
                chunk_text_preview: row.get(7)?,
                score: row.get::<_, f64>(8)? as f32,
                rank: row.get::<_, i64>(9)? as u32,
                contributed: row.get::<_, i32>(10)? != 0,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── Audit Export & Analytics ────────────────────────────────────────

    fn export_audit_logs(&self, filter: &super::AuditLogFilter) -> Vec<serde_json::Value> {
        // Audit log is stored as JSON in settings key "audit_log"
        let raw: Vec<serde_json::Value> = self
            .get_setting("audit_log")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        let limit = filter.limit.unwrap_or(1000);
        let offset = filter.offset.unwrap_or(0);

        let mut filtered: Vec<serde_json::Value> = raw
            .into_iter()
            .filter(|e| {
                if let Some(from) = &filter.from
                    && e["timestamp"].as_str().unwrap_or("") < from.as_str()
                {
                    return false;
                }
                if let Some(to) = &filter.to
                    && e["timestamp"].as_str().unwrap_or("") > to.as_str()
                {
                    return false;
                }
                if let Some(uid) = &filter.user_id
                    && e["actor"].as_str().unwrap_or("") != uid.as_str()
                {
                    return false;
                }
                if let Some(action) = &filter.action
                    && e["action"].as_str().unwrap_or("") != action.as_str()
                {
                    return false;
                }
                true
            })
            .collect();

        filtered.reverse();
        if offset < filtered.len() {
            filtered = filtered[offset..].to_vec();
        } else {
            filtered.clear();
        }
        filtered.truncate(limit);
        filtered
    }

    fn get_audit_analytics(&self, filter: &super::AuditLogFilter) -> super::AuditAnalytics {
        use std::collections::HashMap;
        let entries = self.export_audit_logs(&super::AuditLogFilter {
            from: filter.from.clone(),
            to: filter.to.clone(),
            user_id: filter.user_id.clone(),
            action: filter.action.clone(),
            limit: None,
            offset: None,
        });

        let total_events = entries.len() as u64;
        let mut by_type: HashMap<String, u64> = HashMap::new();
        let mut by_user: HashMap<String, u64> = HashMap::new();
        let mut by_day: HashMap<String, u64> = HashMap::new();

        for e in &entries {
            let action = e["action"].as_str().unwrap_or("unknown").to_string();
            *by_type.entry(action).or_insert(0) += 1;
            let actor = e["actor"].as_str().unwrap_or("unknown").to_string();
            *by_user.entry(actor).or_insert(0) += 1;
            let ts = e["timestamp"].as_str().unwrap_or("");
            let day = ts.get(..10).unwrap_or(ts).to_string();
            *by_day.entry(day).or_insert(0) += 1;
        }

        let mut actions_by_type: Vec<(String, u64)> = by_type.into_iter().collect();
        actions_by_type.sort_by(|a, b| b.1.cmp(&a.1));

        let mut actions_by_user: Vec<(String, u64)> = by_user.into_iter().collect();
        actions_by_user.sort_by(|a, b| b.1.cmp(&a.1));

        let mut events_per_day: Vec<(String, u64)> = by_day.into_iter().collect();
        events_per_day.sort_by(|a, b| a.0.cmp(&b.0));

        super::AuditAnalytics {
            total_events,
            actions_by_type,
            actions_by_user,
            events_per_day,
        }
    }

    // ── Personal Memory Persistence ────────────────────────────────────

    fn insert_personal_memory(&self, memory: &super::PersonalMemoryRow) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO personal_memories
             (id, user_id, memory_type, summary, topics, importance, relevance_score, created_at, last_accessed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                memory.id,
                memory.user_id,
                memory.memory_type,
                memory.summary,
                memory.topics,
                memory.importance as f64,
                memory.relevance_score as f64,
                memory.created_at,
                memory.last_accessed_at,
            ],
        )
        .ok();
    }

    fn list_personal_memories(&self, user_id: &str, limit: usize) -> Vec<super::PersonalMemoryRow> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT id, user_id, memory_type, summary, topics, importance, relevance_score,
             created_at, last_accessed_at
             FROM personal_memories WHERE user_id = ?1
             ORDER BY importance DESC LIMIT {limit}"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![user_id], |row| {
            Ok(super::PersonalMemoryRow {
                id: row.get(0)?,
                user_id: row.get(1)?,
                memory_type: row.get(2)?,
                summary: row.get(3)?,
                topics: row.get(4)?,
                importance: row.get::<_, f64>(5)? as f32,
                relevance_score: row.get::<_, f64>(6)? as f32,
                created_at: row.get(7)?,
                last_accessed_at: row.get(8)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_personal_memory(&self, memory_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute(
                "DELETE FROM personal_memories WHERE id = ?1",
                params![memory_id],
            )
            .map_err(|e| ThaiRagError::Internal(e.to_string()))?;
        if rows == 0 {
            Err(ThaiRagError::NotFound(format!(
                "Memory {memory_id} not found"
            )))
        } else {
            Ok(())
        }
    }

    fn delete_all_personal_memories(&self, user_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM personal_memories WHERE user_id = ?1",
            params![user_id],
        )
        .map_err(|e| ThaiRagError::Internal(e.to_string()))?;
        Ok(())
    }

    fn count_personal_memories(&self, user_id: &str) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM personal_memories WHERE user_id = ?1",
            params![user_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    // ── Multi-tenancy ───────────────────────────────────────────────────

    fn insert_tenant(&self, name: String, plan: String) -> Result<super::Tenant> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tenants (id, name, plan, is_active, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
            params![id, name, plan, now],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_tenant: {e}")))?;
        Ok(super::Tenant {
            id,
            name,
            plan,
            is_active: true,
            created_at: now,
        })
    }

    fn get_tenant(&self, id: &str) -> Result<super::Tenant> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, plan, is_active, created_at FROM tenants WHERE id = ?1",
            params![id],
            |row| {
                Ok(super::Tenant {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    plan: row.get(2)?,
                    is_active: row.get::<_, i64>(3)? != 0,
                    created_at: row.get(4)?,
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Tenant {id} not found")))
    }

    fn list_tenants(&self) -> Vec<super::Tenant> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, plan, is_active, created_at FROM tenants ORDER BY created_at",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok(super::Tenant {
                id: row.get(0)?,
                name: row.get(1)?,
                plan: row.get(2)?,
                is_active: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn update_tenant(&self, id: &str, name: String, plan: String) -> Result<super::Tenant> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "UPDATE tenants SET name = ?2, plan = ?3 WHERE id = ?1",
                params![id, name, plan],
            )
            .map_err(|e| ThaiRagError::Internal(format!("update_tenant: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!("Tenant {id} not found")));
        }
        let created_at: String = conn
            .query_row(
                "SELECT created_at FROM tenants WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or_default();
        Ok(super::Tenant {
            id: id.to_string(),
            name,
            plan,
            is_active: true,
            created_at,
        })
    }

    fn delete_tenant(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM tenants WHERE id = ?1", params![id])
            .map_err(|e| ThaiRagError::Internal(format!("delete_tenant: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!("Tenant {id} not found")));
        }
        Ok(())
    }

    fn get_tenant_quota(&self, id: &str) -> super::TenantQuota {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT max_documents, max_storage_bytes, max_queries_per_day, max_users, max_workspaces FROM tenant_quotas WHERE tenant_id = ?1",
            params![id],
            |row| {
                Ok(super::TenantQuota {
                    max_documents: row.get::<_, i64>(0)? as u64,
                    max_storage_bytes: row.get::<_, i64>(1)? as u64,
                    max_queries_per_day: row.get::<_, i64>(2)? as u64,
                    max_users: row.get::<_, i64>(3)? as u64,
                    max_workspaces: row.get::<_, i64>(4)? as u64,
                })
            },
        )
        .unwrap_or_default()
    }

    fn set_tenant_quota(&self, id: &str, quota: &super::TenantQuota) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tenant_quotas (tenant_id, max_documents, max_storage_bytes, max_queries_per_day, max_users, max_workspaces) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(tenant_id) DO UPDATE SET max_documents=?2, max_storage_bytes=?3, max_queries_per_day=?4, max_users=?5, max_workspaces=?6",
            params![
                id,
                quota.max_documents as i64,
                quota.max_storage_bytes as i64,
                quota.max_queries_per_day as i64,
                quota.max_users as i64,
                quota.max_workspaces as i64,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("set_tenant_quota: {e}")))?;
        Ok(())
    }

    fn get_tenant_usage(&self, id: &str) -> super::TenantUsage {
        let conn = self.conn.lock().unwrap();

        // Documents via workspace → dept → org → tenant mapping
        let current_documents: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM documents d JOIN workspaces w ON d.workspace_id = w.id JOIN departments dp ON w.dept_id = dp.id JOIN tenant_org_mapping tom ON dp.org_id = tom.org_id WHERE tom.tenant_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let current_storage_bytes: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(d.size_bytes), 0) FROM documents d JOIN workspaces w ON d.workspace_id = w.id JOIN departments dp ON w.dept_id = dp.id JOIN tenant_org_mapping tom ON dp.org_id = tom.org_id WHERE tom.tenant_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let current_workspaces: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspaces w JOIN departments dp ON w.dept_id = dp.id JOIN tenant_org_mapping tom ON dp.org_id = tom.org_id WHERE tom.tenant_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let current_users: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .unwrap_or(0);

        let queries_today: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM search_analytics_events sae \
                 JOIN workspaces w ON sae.workspace_id = w.id \
                 JOIN departments dp ON w.dept_id = dp.id \
                 JOIN tenant_org_mapping tom ON dp.org_id = tom.org_id \
                 WHERE tom.tenant_id = ?1 AND sae.timestamp >= date('now')",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        super::TenantUsage {
            current_documents: current_documents as u64,
            current_storage_bytes: current_storage_bytes as u64,
            queries_today: queries_today as u64,
            current_users: current_users as u64,
            current_workspaces: current_workspaces as u64,
        }
    }

    fn assign_org_to_tenant(&self, org_id: OrgId, tenant_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tenant_org_mapping (org_id, tenant_id) VALUES (?1, ?2) ON CONFLICT(org_id) DO UPDATE SET tenant_id=?2",
            params![org_id.0.to_string(), tenant_id],
        )
        .map_err(|e| ThaiRagError::Internal(format!("assign_org_to_tenant: {e}")))?;
        Ok(())
    }

    fn get_tenant_for_org(&self, org_id: OrgId) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT tenant_id FROM tenant_org_mapping WHERE org_id = ?1",
            params![org_id.0.to_string()],
            |row| row.get(0),
        )
        .ok()
    }

    // ── RBAC v2 ─────────────────────────────────────────────────────────

    fn insert_custom_role(&self, role: &super::CustomRole) -> Result<super::CustomRole> {
        let conn = self.conn.lock().unwrap();
        let perms = serde_json::to_string(&role.permissions).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO custom_roles (id, name, description, permissions, is_system, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                role.id,
                role.name,
                role.description,
                perms,
                role.is_system as i64,
                role.created_at,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_custom_role: {e}")))?;
        Ok(role.clone())
    }

    fn get_custom_role(&self, id: &str) -> Result<super::CustomRole> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, description, permissions, is_system, created_at FROM custom_roles WHERE id = ?1",
            params![id],
            |row| {
                let perms_s: String = row.get(3)?;
                let permissions = serde_json::from_str(&perms_s).unwrap_or_default();
                Ok(super::CustomRole {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    permissions,
                    is_system: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Custom role {id} not found")))
    }

    fn list_custom_roles(&self) -> Vec<super::CustomRole> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, description, permissions, is_system, created_at FROM custom_roles ORDER BY created_at")
            .unwrap();
        stmt.query_map([], |row| {
            let perms_s: String = row.get(3)?;
            let permissions = serde_json::from_str(&perms_s).unwrap_or_default();
            Ok(super::CustomRole {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                permissions,
                is_system: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn update_custom_role(&self, role: &super::CustomRole) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let perms = serde_json::to_string(&role.permissions).unwrap_or_else(|_| "[]".to_string());
        let n = conn
            .execute(
                "UPDATE custom_roles SET name = ?2, description = ?3, permissions = ?4 WHERE id = ?1",
                params![role.id, role.name, role.description, perms],
            )
            .map_err(|e| ThaiRagError::Internal(format!("update_custom_role: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Custom role {} not found",
                role.id
            )));
        }
        Ok(())
    }

    fn delete_custom_role(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM custom_roles WHERE id = ?1", params![id])
            .map_err(|e| ThaiRagError::Internal(format!("delete_custom_role: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Custom role {id} not found"
            )));
        }
        Ok(())
    }

    // ── Document Collaboration ──────────────────────────────────────────

    fn insert_comment(&self, comment: &super::DocumentComment) -> Result<super::DocumentComment> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO document_comments (id, doc_id, user_id, user_name, text, parent_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                comment.id,
                comment.doc_id,
                comment.user_id,
                comment.user_name,
                comment.text,
                comment.parent_id,
                comment.created_at,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_comment: {e}")))?;
        Ok(comment.clone())
    }

    fn list_comments(&self, doc_id: &str) -> Vec<super::DocumentComment> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, doc_id, user_id, user_name, text, parent_id, created_at FROM document_comments WHERE doc_id = ?1 ORDER BY created_at")
            .unwrap();
        stmt.query_map(params![doc_id], |row| {
            Ok(super::DocumentComment {
                id: row.get(0)?,
                doc_id: row.get(1)?,
                user_id: row.get(2)?,
                user_name: row.get(3)?,
                text: row.get(4)?,
                parent_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn delete_comment(&self, comment_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "DELETE FROM document_comments WHERE id = ?1",
                params![comment_id],
            )
            .map_err(|e| ThaiRagError::Internal(format!("delete_comment: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Comment {comment_id} not found"
            )));
        }
        Ok(())
    }

    fn insert_annotation(
        &self,
        annotation: &super::DocumentAnnotation,
    ) -> Result<super::DocumentAnnotation> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO document_annotations (id, doc_id, user_id, user_name, chunk_id, text, highlight_start, highlight_end, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                annotation.id,
                annotation.doc_id,
                annotation.user_id,
                annotation.user_name,
                annotation.chunk_id,
                annotation.text,
                annotation.highlight_start.map(|v| v as i64),
                annotation.highlight_end.map(|v| v as i64),
                annotation.created_at,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_annotation: {e}")))?;
        Ok(annotation.clone())
    }

    fn list_annotations(&self, doc_id: &str) -> Vec<super::DocumentAnnotation> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, doc_id, user_id, user_name, chunk_id, text, highlight_start, highlight_end, created_at FROM document_annotations WHERE doc_id = ?1 ORDER BY created_at")
            .unwrap();
        stmt.query_map(params![doc_id], |row| {
            Ok(super::DocumentAnnotation {
                id: row.get(0)?,
                doc_id: row.get(1)?,
                user_id: row.get(2)?,
                user_name: row.get(3)?,
                chunk_id: row.get(4)?,
                text: row.get(5)?,
                highlight_start: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
                highlight_end: row.get::<_, Option<i64>>(7)?.map(|v| v as u32),
                created_at: row.get(8)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn delete_annotation(&self, annotation_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "DELETE FROM document_annotations WHERE id = ?1",
                params![annotation_id],
            )
            .map_err(|e| ThaiRagError::Internal(format!("delete_annotation: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Annotation {annotation_id} not found"
            )));
        }
        Ok(())
    }

    fn insert_review(&self, review: &super::DocumentReview) -> Result<super::DocumentReview> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO document_reviews (id, doc_id, reviewer_id, reviewer_name, status, comments, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                review.id,
                review.doc_id,
                review.reviewer_id,
                review.reviewer_name,
                review.status,
                review.comments,
                review.created_at,
                review.updated_at,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_review: {e}")))?;
        Ok(review.clone())
    }

    fn list_reviews(&self, doc_id: &str) -> Vec<super::DocumentReview> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, doc_id, reviewer_id, reviewer_name, status, comments, created_at, updated_at FROM document_reviews WHERE doc_id = ?1 ORDER BY created_at")
            .unwrap();
        stmt.query_map(params![doc_id], |row| {
            Ok(super::DocumentReview {
                id: row.get(0)?,
                doc_id: row.get(1)?,
                reviewer_id: row.get(2)?,
                reviewer_name: row.get(3)?,
                status: row.get(4)?,
                comments: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn update_review_status(
        &self,
        review_id: &str,
        status: &str,
        comments: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let n = conn
            .execute(
                "UPDATE document_reviews SET status = ?2, comments = COALESCE(?3, comments), updated_at = ?4 WHERE id = ?1",
                params![review_id, status, comments, now],
            )
            .map_err(|e| ThaiRagError::Internal(format!("update_review_status: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Review {review_id} not found"
            )));
        }
        Ok(())
    }

    // ── Search Quality Regression ───────────────────────────────────────

    fn insert_regression_run(&self, run: &super::RegressionRun) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO regression_runs (id, timestamp, query_set_id, baseline_score, current_score, degradation, passed, details) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.id,
                run.timestamp,
                run.query_set_id,
                run.baseline_score,
                run.current_score,
                run.degradation,
                run.passed as i64,
                run.details,
            ],
        );
    }

    fn list_regression_runs(&self, limit: usize) -> Vec<super::RegressionRun> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, timestamp, query_set_id, baseline_score, current_score, degradation, passed, details FROM regression_runs ORDER BY timestamp DESC LIMIT ?1")
            .unwrap();
        stmt.query_map(params![limit as i64], |row| {
            Ok(super::RegressionRun {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                query_set_id: row.get(2)?,
                baseline_score: row.get(3)?,
                current_score: row.get(4)?,
                degradation: row.get(5)?,
                passed: row.get::<_, i64>(6)? != 0,
                details: row.get(7)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    // ── Prompt Marketplace ──────────────────────────────────────────────

    fn insert_prompt_template(
        &self,
        template: &super::PromptTemplate,
    ) -> Result<super::PromptTemplate> {
        let conn = self.conn.lock().unwrap();
        let vars = serde_json::to_string(&template.variables).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO prompt_templates (id, name, description, category, content, variables, author_id, author_name, version, is_public, rating_avg, rating_count, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                template.id,
                template.name,
                template.description,
                template.category,
                template.content,
                vars,
                template.author_id,
                template.author_name,
                template.version as i64,
                template.is_public as i64,
                template.rating_avg,
                template.rating_count as i64,
                template.created_at,
                template.updated_at,
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_prompt_template: {e}")))?;
        Ok(template.clone())
    }

    fn list_prompt_templates(
        &self,
        filter: &super::PromptTemplateFilter,
    ) -> Vec<super::PromptTemplate> {
        let conn = self.conn.lock().unwrap();
        // Build dynamic query
        let mut conditions: Vec<String> = Vec::new();
        if filter.category.is_some() {
            conditions.push("category = ?1".to_string());
        }
        if filter.is_public.is_some() {
            conditions.push(format!("is_public = ?{}", conditions.len() + 1));
        }
        if filter.author_id.is_some() {
            conditions.push(format!("author_id = ?{}", conditions.len() + 1));
        }
        if filter.search.is_some() {
            let n = conditions.len() + 1;
            conditions.push(format!(
                "(name LIKE ?{n} OR description LIKE ?{n} OR content LIKE ?{n})"
            ));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let limit = filter.limit.unwrap_or(100);
        let offset = filter.offset.unwrap_or(0);
        let sql = format!(
            "SELECT id, name, description, category, content, variables, author_id, author_name, version, is_public, rating_avg, rating_count, created_at, updated_at FROM prompt_templates {where_clause} ORDER BY created_at DESC LIMIT {limit} OFFSET {offset}"
        );

        // We use a simple approach — collect params manually
        let mut param_values: Vec<String> = Vec::new();
        if let Some(ref cat) = filter.category {
            param_values.push(cat.clone());
        }
        if let Some(pub_flag) = filter.is_public {
            param_values.push(if pub_flag {
                "1".to_string()
            } else {
                "0".to_string()
            });
        }
        if let Some(ref aid) = filter.author_id {
            param_values.push(aid.clone());
        }
        if let Some(ref s) = filter.search {
            param_values.push(format!("%{s}%"));
        }

        let rows: Vec<super::PromptTemplate> = match param_values.len() {
            0 => {
                let mut stmt = conn.prepare(&sql).unwrap();
                stmt.query_map([], row_to_prompt_template)
                    .unwrap()
                    .flatten()
                    .collect()
            }
            1 => {
                let mut stmt = conn.prepare(&sql).unwrap();
                stmt.query_map(params![param_values[0]], row_to_prompt_template)
                    .unwrap()
                    .flatten()
                    .collect()
            }
            2 => {
                let mut stmt = conn.prepare(&sql).unwrap();
                stmt.query_map(
                    params![param_values[0], param_values[1]],
                    row_to_prompt_template,
                )
                .unwrap()
                .flatten()
                .collect()
            }
            3 => {
                let mut stmt = conn.prepare(&sql).unwrap();
                stmt.query_map(
                    params![param_values[0], param_values[1], param_values[2]],
                    row_to_prompt_template,
                )
                .unwrap()
                .flatten()
                .collect()
            }
            _ => {
                let mut stmt = conn.prepare(&sql).unwrap();
                stmt.query_map(
                    params![
                        param_values[0],
                        param_values[1],
                        param_values[2],
                        param_values[3]
                    ],
                    row_to_prompt_template,
                )
                .unwrap()
                .flatten()
                .collect()
            }
        };
        rows
    }

    fn get_prompt_template(&self, id: &str) -> Result<super::PromptTemplate> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, description, category, content, variables, author_id, author_name, version, is_public, rating_avg, rating_count, created_at, updated_at FROM prompt_templates WHERE id = ?1",
            params![id],
            row_to_prompt_template,
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Prompt template {id} not found")))
    }

    fn update_prompt_template(&self, template: &super::PromptTemplate) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let vars = serde_json::to_string(&template.variables).unwrap_or_else(|_| "[]".to_string());
        let now = Utc::now().to_rfc3339();
        let n = conn
            .execute(
                "UPDATE prompt_templates SET name=?2, description=?3, category=?4, content=?5, variables=?6, is_public=?7, version=?8, updated_at=?9 WHERE id=?1",
                params![
                    template.id,
                    template.name,
                    template.description,
                    template.category,
                    template.content,
                    vars,
                    template.is_public as i64,
                    template.version as i64,
                    now,
                ],
            )
            .map_err(|e| ThaiRagError::Internal(format!("update_prompt_template: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Prompt template {} not found",
                template.id
            )));
        }
        Ok(())
    }

    fn delete_prompt_template(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM prompt_templates WHERE id = ?1", params![id])
            .map_err(|e| ThaiRagError::Internal(format!("delete_prompt_template: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "Prompt template {id} not found"
            )));
        }
        Ok(())
    }

    fn rate_prompt_template(&self, rating: &super::PromptRating) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Upsert rating
        conn.execute(
            "INSERT INTO prompt_ratings (template_id, user_id, rating) VALUES (?1, ?2, ?3) ON CONFLICT(template_id, user_id) DO UPDATE SET rating=?3",
            params![rating.template_id, rating.user_id, rating.rating as i64],
        )
        .map_err(|e| ThaiRagError::Internal(format!("rate_prompt_template: {e}")))?;
        // Recompute avg
        let (avg, count): (f64, i64) = conn
            .query_row(
                "SELECT AVG(CAST(rating AS REAL)), COUNT(*) FROM prompt_ratings WHERE template_id = ?1",
                params![rating.template_id],
                |row| Ok((row.get(0).unwrap_or(0.0), row.get(1).unwrap_or(0))),
            )
            .unwrap_or((0.0, 0));
        conn.execute(
            "UPDATE prompt_templates SET rating_avg=?2, rating_count=?3 WHERE id=?1",
            params![rating.template_id, avg, count],
        )
        .map_err(|e| ThaiRagError::Internal(format!("rate_prompt_template update: {e}")))?;
        Ok(())
    }

    fn fork_prompt_template(
        &self,
        id: &str,
        user_id: &str,
        user_name: &str,
    ) -> Result<super::PromptTemplate> {
        let original = self.get_prompt_template(id)?;
        let now = Utc::now().to_rfc3339();
        let forked = super::PromptTemplate {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("{} (fork)", original.name),
            description: original.description.clone(),
            category: original.category.clone(),
            content: original.content.clone(),
            variables: original.variables.clone(),
            author_id: Some(user_id.to_string()),
            author_name: Some(user_name.to_string()),
            version: 1,
            is_public: false,
            rating_avg: 0.0,
            rating_count: 0,
            created_at: now.clone(),
            updated_at: now,
        };
        self.insert_prompt_template(&forked)
    }

    // ── Embedding Fine-tuning ───────────────────────────────────────────

    fn insert_training_dataset(
        &self,
        name: String,
        description: String,
    ) -> Result<super::TrainingDataset> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO training_datasets (id, name, description, pair_count, created_at) VALUES (?1, ?2, ?3, 0, ?4)",
            params![id, name, description, now],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_training_dataset: {e}")))?;
        Ok(super::TrainingDataset {
            id,
            name,
            description,
            pair_count: 0,
            created_at: now,
        })
    }

    fn list_training_datasets(&self) -> Vec<super::TrainingDataset> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, description, pair_count, created_at FROM training_datasets ORDER BY created_at DESC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(super::TrainingDataset {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                pair_count: row.get::<_, i64>(3)? as u32,
                created_at: row.get(4)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn get_training_dataset(&self, id: &str) -> Result<super::TrainingDataset> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, description, pair_count, created_at FROM training_datasets WHERE id = ?1",
            params![id],
            |row| {
                Ok(super::TrainingDataset {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    pair_count: row.get::<_, i64>(3)? as u32,
                    created_at: row.get(4)?,
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("TrainingDataset {id} not found")))
    }

    fn delete_training_dataset(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM training_datasets WHERE id = ?1", params![id])
            .map_err(|e| ThaiRagError::Internal(format!("delete_training_dataset: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "TrainingDataset {id} not found"
            )));
        }
        Ok(())
    }

    fn insert_training_pair(&self, pair: &super::TrainingPair) -> Result<super::TrainingPair> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO training_pairs (id, dataset_id, query, positive_doc, negative_doc, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![pair.id, pair.dataset_id, pair.query, pair.positive_doc, pair.negative_doc, pair.created_at],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_training_pair: {e}")))?;
        conn.execute(
            "UPDATE training_datasets SET pair_count = pair_count + 1 WHERE id = ?1",
            params![pair.dataset_id],
        )
        .map_err(|e| ThaiRagError::Internal(format!("update pair_count: {e}")))?;
        Ok(pair.clone())
    }

    fn list_training_pairs(&self, dataset_id: &str) -> Vec<super::TrainingPair> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, dataset_id, query, positive_doc, negative_doc, created_at FROM training_pairs WHERE dataset_id = ?1 ORDER BY created_at")
            .unwrap();
        stmt.query_map(params![dataset_id], |row| {
            Ok(super::TrainingPair {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                query: row.get(2)?,
                positive_doc: row.get(3)?,
                negative_doc: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn delete_training_pair(&self, pair_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Get dataset_id before deleting
        let dataset_id: Option<String> = conn
            .query_row(
                "SELECT dataset_id FROM training_pairs WHERE id = ?1",
                params![pair_id],
                |row| row.get(0),
            )
            .ok();
        let n = conn
            .execute("DELETE FROM training_pairs WHERE id = ?1", params![pair_id])
            .map_err(|e| ThaiRagError::Internal(format!("delete_training_pair: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "TrainingPair {pair_id} not found"
            )));
        }
        if let Some(did) = dataset_id {
            let _ = conn.execute(
                "UPDATE training_datasets SET pair_count = MAX(0, pair_count - 1) WHERE id = ?1",
                params![did],
            );
        }
        Ok(())
    }

    fn insert_finetune_job(&self, job: &super::FinetuneJob) -> Result<super::FinetuneJob> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO finetune_jobs (id, dataset_id, base_model, status, metrics, output_model_path, config, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![job.id, job.dataset_id, job.base_model, job.status, job.metrics, job.output_model_path, job.config, job.created_at, job.updated_at],
        )
        .map_err(|e| ThaiRagError::Internal(format!("insert_finetune_job: {e}")))?;
        Ok(job.clone())
    }

    fn get_finetune_job(&self, id: &str) -> Result<super::FinetuneJob> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, dataset_id, base_model, status, metrics, output_model_path, config, created_at, updated_at FROM finetune_jobs WHERE id = ?1",
            params![id],
            |row| {
                Ok(super::FinetuneJob {
                    id: row.get(0)?,
                    dataset_id: row.get(1)?,
                    base_model: row.get(2)?,
                    status: row.get(3)?,
                    metrics: row.get(4)?,
                    output_model_path: row.get(5)?,
                    config: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("FinetuneJob {id} not found")))
    }

    fn list_finetune_jobs(&self) -> Vec<super::FinetuneJob> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, dataset_id, base_model, status, metrics, output_model_path, config, created_at, updated_at FROM finetune_jobs ORDER BY created_at DESC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(super::FinetuneJob {
                id: row.get(0)?,
                dataset_id: row.get(1)?,
                base_model: row.get(2)?,
                status: row.get(3)?,
                metrics: row.get(4)?,
                output_model_path: row.get(5)?,
                config: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    fn update_finetune_job_status(
        &self,
        id: &str,
        status: &str,
        metrics: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let n = conn
            .execute(
                "UPDATE finetune_jobs SET status = ?2, metrics = COALESCE(?3, metrics), updated_at = ?4 WHERE id = ?1",
                params![id, status, metrics, now],
            )
            .map_err(|e| ThaiRagError::Internal(format!("update_finetune_job_status: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "FinetuneJob {id} not found"
            )));
        }
        Ok(())
    }

    fn update_finetune_job_full(
        &self,
        id: &str,
        status: &str,
        metrics: Option<&str>,
        output_model_path: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let n = conn
            .execute(
                "UPDATE finetune_jobs SET status = ?2, metrics = COALESCE(?3, metrics), output_model_path = COALESCE(?4, output_model_path), updated_at = ?5 WHERE id = ?1",
                params![id, status, metrics, output_model_path, now],
            )
            .map_err(|e| ThaiRagError::Internal(format!("update_finetune_job_full: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "FinetuneJob {id} not found"
            )));
        }
        Ok(())
    }

    fn delete_finetune_job(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM finetune_jobs WHERE id = ?1", params![id])
            .map_err(|e| ThaiRagError::Internal(format!("delete_finetune_job: {e}")))?;
        if n == 0 {
            return Err(ThaiRagError::NotFound(format!(
                "FinetuneJob {id} not found"
            )));
        }
        Ok(())
    }
}

fn row_to_prompt_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<super::PromptTemplate> {
    let vars_s: String = row.get(5)?;
    let variables: Vec<String> = serde_json::from_str(&vars_s).unwrap_or_default();
    Ok(super::PromptTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        category: row.get(3)?,
        content: row.get(4)?,
        variables,
        author_id: row.get(6)?,
        author_name: row.get(7)?,
        version: row.get::<_, i64>(8)? as u32,
        is_public: row.get::<_, i64>(9)? != 0,
        rating_avg: row.get(10)?,
        rating_count: row.get::<_, i64>(11)? as u32,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
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
            version: 1,
            content_hash: None,
            source_url: None,
            refresh_schedule: None,
            last_refreshed_at: None,
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
            version: 1,
            content_hash: None,
            source_url: None,
            refresh_schedule: None,
            last_refreshed_at: None,
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

    #[test]
    fn workspace_acl_grant_list_revoke() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
        let user = store
            .insert_user("u@t.com".into(), "U".into(), "h".into())
            .unwrap();
        let granter = store
            .insert_user("admin@t.com".into(), "Admin".into(), "h".into())
            .unwrap();

        // Grant read
        let acl = store
            .grant_workspace_access(user.id, ws.id, AclPermission::Read, Some(granter.id))
            .unwrap();
        assert_eq!(acl.permission, AclPermission::Read);
        assert_eq!(acl.granted_by, Some(granter.id));

        // List
        let acls = store.list_workspace_acls(ws.id);
        assert_eq!(acls.len(), 1);
        assert_eq!(acls[0].user_id, user.id);

        // Get permission
        assert_eq!(
            store.get_user_workspace_acl(user.id, ws.id),
            Some(AclPermission::Read)
        );

        // Upgrade to write (upsert)
        store
            .grant_workspace_access(user.id, ws.id, AclPermission::Write, Some(granter.id))
            .unwrap();
        assert_eq!(
            store.get_user_workspace_acl(user.id, ws.id),
            Some(AclPermission::Write)
        );
        // Should still be 1 entry (upsert, not duplicate)
        assert_eq!(store.list_workspace_acls(ws.id).len(), 1);

        // List accessible workspaces
        let ws_ids = store.list_accessible_workspaces(user.id);
        assert!(ws_ids.contains(&ws.id));

        // Revoke
        store.revoke_workspace_access(user.id, ws.id).unwrap();
        assert_eq!(store.get_user_workspace_acl(user.id, ws.id), None);
        assert_eq!(store.list_workspace_acls(ws.id).len(), 0);

        // Revoke again should fail
        assert!(store.revoke_workspace_access(user.id, ws.id).is_err());
    }

    #[test]
    fn document_acl_grant_check_revoke() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
        let user = store
            .insert_user("u@t.com".into(), "U".into(), "h".into())
            .unwrap();
        let now = Utc::now();
        let doc = Document {
            id: DocId::new(),
            workspace_id: ws.id,
            title: "secret".into(),
            mime_type: "text/plain".into(),
            size_bytes: 10,
            status: DocStatus::Ready,
            chunk_count: 0,
            error_message: None,
            processing_step: None,
            version: 1,
            content_hash: None,
            source_url: None,
            refresh_schedule: None,
            last_refreshed_at: None,
            created_at: now,
            updated_at: now,
        };
        let doc = store.insert_document(doc).unwrap();

        // No access initially
        assert_eq!(store.check_document_access(user.id, doc.id), None);

        // Grant read
        let acl = store
            .grant_document_access(user.id, doc.id, AclPermission::Read)
            .unwrap();
        assert_eq!(acl.permission, AclPermission::Read);

        // Check
        assert_eq!(
            store.check_document_access(user.id, doc.id),
            Some(AclPermission::Read)
        );

        // Upgrade to write (upsert)
        store
            .grant_document_access(user.id, doc.id, AclPermission::Write)
            .unwrap();
        assert_eq!(
            store.check_document_access(user.id, doc.id),
            Some(AclPermission::Write)
        );

        // Revoke
        store.revoke_document_access(user.id, doc.id).unwrap();
        assert_eq!(store.check_document_access(user.id, doc.id), None);

        // Revoke again should fail
        assert!(store.revoke_document_access(user.id, doc.id).is_err());
    }

    #[test]
    fn acl_permission_ordering() {
        assert!(AclPermission::Read < AclPermission::Write);
        assert!(AclPermission::Write < AclPermission::Admin);
        assert!(AclPermission::Read < AclPermission::Admin);
    }
}
