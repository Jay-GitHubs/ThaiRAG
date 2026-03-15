use std::future::Future;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use thairag_core::models::{
    Department, DocStatus, Document, IdentityProvider, Organization, PermissionScope, User,
    UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{DeptId, DocId, IdpId, OrgId, UserId, WorkspaceId};
use thairag_core::ThaiRagError;
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
    pub async fn new(db_url: &str, max_connections: u32) -> std::result::Result<Self, ThaiRagError> {
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
            return Err(ThaiRagError::NotFound(format!(
                "Department {id} not found"
            )));
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
            return Err(ThaiRagError::NotFound(format!(
                "Workspace {id} not found"
            )));
        }
        Ok(())
    }

    // ── Document ────────────────────────────────────────────────────

    fn insert_document(&self, doc: Document) -> Result<Document> {
        self.get_workspace(doc.workspace_id)?;
        block_on(sqlx::query(
            "INSERT INTO documents (id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
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
        .bind(doc.created_at)
        .bind(doc.updated_at)
        .execute(&self.pool))
        .map_err(|e| ThaiRagError::Internal(format!("Postgres insert document: {e}")))?;
        Ok(doc)
    }

    fn get_document(&self, id: DocId) -> Result<Document> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, String, i64, String, i32, Option<String>, Option<String>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, created_at, updated_at FROM documents WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, ws_id, title, mime, size, status, chunks, err_msg, processing_step, ca, ua)| Document {
            id: DocId(id),
            workspace_id: WorkspaceId(ws_id),
            title,
            mime_type: mime,
            size_bytes: size,
            status: DocStatus::from_str_lossy(&status),
            chunk_count: chunks as i64,
            error_message: err_msg,
            processing_step,
            created_at: ca,
            updated_at: ua,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        block_on(
            sqlx::query_as::<_, (Uuid, Uuid, String, String, i64, String, i32, Option<String>, Option<String>, DateTime<Utc>, DateTime<Utc>)>(
                "SELECT id, workspace_id, title, mime_type, size_bytes, status, chunk_count, error_message, processing_step, created_at, updated_at FROM documents WHERE workspace_id = $1",
            )
            .bind(workspace_id.0)
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, ws_id, title, mime, size, status, chunks, err_msg, processing_step, ca, ua)| Document {
            id: DocId(id),
            workspace_id: WorkspaceId(ws_id),
            title,
            mime_type: mime,
            size_bytes: size,
            status: DocStatus::from_str_lossy(&status),
            chunk_count: chunks as i64,
            error_message: err_msg,
            processing_step,
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
            return Err(ThaiRagError::NotFound(format!(
                "Document {id} not found"
            )));
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
            created_at: Utc::now(),
        };
        block_on(sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(user.id.0)
        .bind(&user.email)
        .bind(&user.name)
        .bind(&password_hash)
        .bind(&user.auth_provider)
        .bind(&user.external_id)
        .bind(user.is_super_admin)
        .bind(&user.role)
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
            created_at: Utc::now(),
        };
        block_on(sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(user.id.0)
        .bind(&user.email)
        .bind(&user.name)
        .bind(&password_hash)
        .bind(&user.auth_provider)
        .bind(&user.external_id)
        .bind(user.is_super_admin)
        .bind(&user.role)
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
            sqlx::query_as::<_, (Uuid, String, String, String, String, Option<String>, bool, String, DateTime<Utc>)>(
                "SELECT id, email, name, password_hash, auth_provider, external_id, is_super_admin, role, created_at FROM users WHERE email = $1",
            )
            .bind(&email_lower)
            .fetch_one(&self.pool),
        )
        .map(|(id, email, name, pw, auth_provider, external_id, is_super_admin, role, ca)| UserRecord {
            user: User {
                id: UserId(id),
                email,
                name,
                auth_provider,
                external_id,
                is_super_admin,
                role,
                created_at: ca,
            }.normalize_role(),
            password_hash: pw,
        })
        .map_err(|_| ThaiRagError::NotFound(format!("User with email {email} not found")))
    }

    fn get_user(&self, id: UserId) -> Result<User> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, String, Option<String>, bool, String, DateTime<Utc>)>(
                "SELECT id, email, name, auth_provider, external_id, is_super_admin, role, created_at FROM users WHERE id = $1",
            )
            .bind(id.0)
            .fetch_one(&self.pool),
        )
        .map(|(id, email, name, auth_provider, external_id, is_super_admin, role, ca)| User {
            id: UserId(id),
            email,
            name,
            auth_provider,
            external_id,
            is_super_admin,
            role,
            created_at: ca,
        }.normalize_role())
        .map_err(|_| ThaiRagError::NotFound(format!("User {id} not found")))
    }

    fn list_users(&self) -> Vec<User> {
        block_on(
            sqlx::query_as::<_, (Uuid, String, String, String, Option<String>, bool, String, DateTime<Utc>)>(
                "SELECT id, email, name, auth_provider, external_id, is_super_admin, role, created_at FROM users",
            )
            .fetch_all(&self.pool),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(id, email, name, auth_provider, external_id, is_super_admin, role, ca)| User {
            id: UserId(id),
            email,
            name,
            auth_provider,
            external_id,
            is_super_admin,
            role,
            created_at: ca,
        }.normalize_role())
        .collect()
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
            return Err(ThaiRagError::NotFound(format!("Identity provider {id} not found")));
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
            return Err(ThaiRagError::NotFound(format!("Identity provider {id} not found")));
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
        block_on(
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool),
        )
        .ok()
        .flatten()
    }

    fn set_setting(&self, key: &str, value: &str) {
        let now = chrono::Utc::now();
        let _ = block_on(
            sqlx::query(
                "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, $3)
                 ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = $3",
            )
            .bind(key)
            .bind(value)
            .bind(now)
            .execute(&self.pool),
        );
    }

    fn delete_setting(&self, key: &str) {
        let _ = block_on(
            sqlx::query("DELETE FROM settings WHERE key = $1")
                .bind(key)
                .execute(&self.pool),
        );
    }
}
