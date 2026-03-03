use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use thairag_core::models::{
    Department, Document, Organization, PermissionScope, User, UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{DeptId, DocId, OrgId, UserId, WorkspaceId};
use thairag_core::ThaiRagError;
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

fn scope_to_parts(scope: &PermissionScope) -> (&str, String, String, String) {
    match scope {
        PermissionScope::Org { org_id } => ("org", org_id.0.to_string(), String::new(), String::new()),
        PermissionScope::Dept { org_id, dept_id } => {
            ("dept", org_id.0.to_string(), dept_id.0.to_string(), String::new())
        }
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
            .execute("DELETE FROM organizations WHERE id = ?1", params![id.0.to_string()])
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete org: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Organization {id} not found")));
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
            .execute("DELETE FROM departments WHERE id = ?1", params![id.0.to_string()])
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

    fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id.0.to_string()])
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
            "INSERT INTO documents (id, workspace_id, title, mime_type, size_bytes, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                doc.id.0.to_string(),
                doc.workspace_id.0.to_string(),
                doc.title,
                doc.mime_type,
                doc.size_bytes,
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
            "SELECT id, workspace_id, title, mime_type, size_bytes, created_at, updated_at FROM documents WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let ws_s: String = row.get(1)?;
                let title: String = row.get(2)?;
                let mime: String = row.get(3)?;
                let size: i64 = row.get(4)?;
                let ca: String = row.get(5)?;
                let ua: String = row.get(6)?;
                Ok(Document {
                    id: DocId(parse_uuid(&id_s)),
                    workspace_id: WorkspaceId(parse_uuid(&ws_s)),
                    title,
                    mime_type: mime,
                    size_bytes: size,
                    created_at: parse_ts(&ca),
                    updated_at: parse_ts(&ua),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, workspace_id, title, mime_type, size_bytes, created_at, updated_at FROM documents WHERE workspace_id = ?1")
            .unwrap();
        stmt.query_map(params![workspace_id.0.to_string()], |row| {
            let id_s: String = row.get(0)?;
            let ws_s: String = row.get(1)?;
            let title: String = row.get(2)?;
            let mime: String = row.get(3)?;
            let size: i64 = row.get(4)?;
            let ca: String = row.get(5)?;
            let ua: String = row.get(6)?;
            Ok(Document {
                id: DocId(parse_uuid(&id_s)),
                workspace_id: WorkspaceId(parse_uuid(&ws_s)),
                title,
                mime_type: mime,
                size_bytes: size,
                created_at: parse_ts(&ca),
                updated_at: parse_ts(&ua),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn delete_document(&self, id: DocId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute("DELETE FROM documents WHERE id = ?1", params![id.0.to_string()])
            .map_err(|e| ThaiRagError::Internal(format!("SQLite delete document: {e}")))?;
        if affected == 0 {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    // ── User ──────────────────────────────────────────────────────────

    fn insert_user(
        &self,
        email: String,
        name: String,
        password_hash: String,
    ) -> Result<User> {
        let email_lower = email.to_lowercase();
        let conn = self.conn.lock().unwrap();

        // Check uniqueness
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
            created_at: Utc::now(),
        };
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                user.id.0.to_string(),
                user.email,
                user.name,
                password_hash,
                ts(&user.created_at),
            ],
        )
        .map_err(|e| ThaiRagError::Internal(format!("SQLite insert user: {e}")))?;
        Ok(user)
    }

    fn get_user_by_email(&self, email: &str) -> Result<UserRecord> {
        let email_lower = email.to_lowercase();
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, email, name, password_hash, created_at FROM users WHERE email = ?1",
            params![email_lower],
            |row| {
                let id_s: String = row.get(0)?;
                let email: String = row.get(1)?;
                let name: String = row.get(2)?;
                let pw: String = row.get(3)?;
                let ca: String = row.get(4)?;
                Ok(UserRecord {
                    user: User {
                        id: UserId(parse_uuid(&id_s)),
                        email,
                        name,
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
            "SELECT id, email, name, created_at FROM users WHERE id = ?1",
            params![id.0.to_string()],
            |row| {
                let id_s: String = row.get(0)?;
                let email: String = row.get(1)?;
                let name: String = row.get(2)?;
                let ca: String = row.get(3)?;
                Ok(User {
                    id: UserId(parse_uuid(&id_s)),
                    email,
                    name,
                    created_at: parse_ts(&ca),
                })
            },
        )
        .map_err(|_| ThaiRagError::NotFound(format!("User {id} not found")))
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
            .query_map(params![user_id.0.to_string(), org_id.0.to_string()], |row| {
                let r: String = row.get(0)?;
                Ok(parse_role(&r))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        roles.into_iter().max()
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

        let record = store.get_user_by_email("alice@example.com").unwrap();
        assert_eq!(record.user.id, user.id);
        assert_eq!(record.password_hash, "hash123");

        let fetched = store.get_user(user.id).unwrap();
        assert_eq!(fetched.name, "Alice");
    }

    #[test]
    fn user_email_uniqueness() {
        let store = mem_store();
        store.insert_user("bob@test.com".into(), "Bob".into(), "h".into()).unwrap();
        let result = store.insert_user("BOB@test.com".into(), "Bob2".into(), "h2".into());
        assert!(result.is_err());
    }

    #[test]
    fn permission_resolution() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();

        let user = store.insert_user("u@test.com".into(), "U".into(), "h".into()).unwrap();

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Viewer,
        });

        assert_eq!(store.get_user_role_for_org(user.id, org.id), Some(Role::Viewer));

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Workspace {
                org_id: org.id,
                dept_id: dept.id,
                workspace_id: ws.id,
            },
            role: Role::Editor,
        });

        assert_eq!(store.get_user_role_for_org(user.id, org.id), Some(Role::Editor));

        let ws_ids = store.get_user_workspace_ids(user.id);
        assert!(ws_ids.contains(&ws.id));
    }

    #[test]
    fn upsert_permission_dedup() {
        let store = mem_store();
        let org = store.insert_org("Acme".into()).unwrap();
        let user = store.insert_user("u@t.com".into(), "U".into(), "h".into()).unwrap();
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
        let user = store.insert_user("o@t.com".into(), "O".into(), "h".into()).unwrap();

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Owner,
        });

        assert_eq!(store.count_org_owners(org.id), 1);
    }
}
