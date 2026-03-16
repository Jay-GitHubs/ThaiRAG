use std::collections::HashMap;
use std::sync::RwLock;

use chrono::Utc;
use thairag_core::ThaiRagError;
use thairag_core::models::{
    Department, DocStatus, Document, IdentityProvider, Organization, PermissionScope, User,
    UserPermission, Workspace,
};
use thairag_core::permission::Role;
use thairag_core::types::{DeptId, DocId, IdpId, OrgId, UserId, WorkspaceId};

use super::{KmStoreTrait, UserRecord, scope_org_id, scopes_match};

type Result<T> = std::result::Result<T, ThaiRagError>;

struct BlobData {
    original_bytes: Option<Vec<u8>>,
    converted_text: Option<String>,
    image_count: i32,
    table_count: i32,
}

pub struct MemoryKmStore {
    orgs: RwLock<HashMap<OrgId, Organization>>,
    depts: RwLock<HashMap<DeptId, Department>>,
    workspaces: RwLock<HashMap<WorkspaceId, Workspace>>,
    documents: RwLock<HashMap<DocId, Document>>,
    document_blobs: RwLock<HashMap<DocId, BlobData>>,
    users: RwLock<HashMap<UserId, UserRecord>>,
    user_by_email: RwLock<HashMap<String, UserId>>,
    permissions: RwLock<Vec<UserPermission>>,
    identity_providers: RwLock<HashMap<IdpId, IdentityProvider>>,
    settings: RwLock<HashMap<String, String>>,
}

impl Default for MemoryKmStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryKmStore {
    pub fn new() -> Self {
        Self {
            orgs: RwLock::new(HashMap::new()),
            depts: RwLock::new(HashMap::new()),
            workspaces: RwLock::new(HashMap::new()),
            documents: RwLock::new(HashMap::new()),
            document_blobs: RwLock::new(HashMap::new()),
            users: RwLock::new(HashMap::new()),
            user_by_email: RwLock::new(HashMap::new()),
            permissions: RwLock::new(Vec::new()),
            identity_providers: RwLock::new(HashMap::new()),
            settings: RwLock::new(HashMap::new()),
        }
    }
}

impl KmStoreTrait for MemoryKmStore {
    // ── Organization ────────────────────────────────────────────────

    fn insert_org(&self, name: String) -> Result<Organization> {
        let now = Utc::now();
        let org = Organization {
            id: OrgId::new(),
            name,
            created_at: now,
            updated_at: now,
        };
        self.orgs.write().unwrap().insert(org.id, org.clone());
        Ok(org)
    }

    fn get_org(&self, id: OrgId) -> Result<Organization> {
        self.orgs
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Organization {id} not found")))
    }

    fn list_orgs(&self) -> Vec<Organization> {
        self.orgs.read().unwrap().values().cloned().collect()
    }

    fn delete_org(&self, id: OrgId) -> Result<()> {
        if self.orgs.write().unwrap().remove(&id).is_none() {
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
        self.depts.write().unwrap().insert(dept.id, dept.clone());
        Ok(dept)
    }

    fn get_dept(&self, id: DeptId) -> Result<Department> {
        self.depts
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Department {id} not found")))
    }

    fn list_depts_in_org(&self, org_id: OrgId) -> Vec<Department> {
        self.depts
            .read()
            .unwrap()
            .values()
            .filter(|d| d.org_id == org_id)
            .cloned()
            .collect()
    }

    fn delete_dept(&self, id: DeptId) -> Result<()> {
        if self.depts.write().unwrap().remove(&id).is_none() {
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
        self.workspaces.write().unwrap().insert(ws.id, ws.clone());
        Ok(ws)
    }

    fn get_workspace(&self, id: WorkspaceId) -> Result<Workspace> {
        self.workspaces
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Workspace {id} not found")))
    }

    fn list_workspaces_in_dept(&self, dept_id: DeptId) -> Vec<Workspace> {
        self.workspaces
            .read()
            .unwrap()
            .values()
            .filter(|w| w.dept_id == dept_id)
            .cloned()
            .collect()
    }

    fn list_workspaces_all(&self) -> Vec<Workspace> {
        self.workspaces.read().unwrap().values().cloned().collect()
    }

    fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
        if self.workspaces.write().unwrap().remove(&id).is_none() {
            return Err(ThaiRagError::NotFound(format!("Workspace {id} not found")));
        }
        Ok(())
    }

    // ── Document ────────────────────────────────────────────────────

    fn insert_document(&self, doc: Document) -> Result<Document> {
        self.get_workspace(doc.workspace_id)?;
        self.documents.write().unwrap().insert(doc.id, doc.clone());
        Ok(doc)
    }

    fn get_document(&self, id: DocId) -> Result<Document> {
        self.documents
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        self.documents
            .read()
            .unwrap()
            .values()
            .filter(|d| d.workspace_id == workspace_id)
            .cloned()
            .collect()
    }

    fn update_document_status(
        &self,
        id: DocId,
        status: DocStatus,
        chunk_count: i64,
        error_message: Option<String>,
    ) -> Result<()> {
        let mut docs = self.documents.write().unwrap();
        let doc = docs
            .get_mut(&id)
            .ok_or_else(|| ThaiRagError::NotFound(format!("Document {id} not found")))?;
        doc.status = status;
        doc.chunk_count = chunk_count;
        doc.error_message = error_message;
        doc.updated_at = chrono::Utc::now();
        Ok(())
    }

    fn update_document_step(&self, id: DocId, step: Option<String>) -> Result<()> {
        let mut docs = self.documents.write().unwrap();
        let doc = docs
            .get_mut(&id)
            .ok_or_else(|| ThaiRagError::NotFound(format!("Document {id} not found")))?;
        doc.processing_step = step;
        doc.updated_at = chrono::Utc::now();
        Ok(())
    }

    fn delete_document(&self, id: DocId) -> Result<()> {
        if self.documents.write().unwrap().remove(&id).is_none() {
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
        let mut blobs = self.document_blobs.write().unwrap();
        let entry = blobs.entry(doc_id).or_insert(BlobData {
            original_bytes: None,
            converted_text: None,
            image_count: 0,
            table_count: 0,
        });
        if original_bytes.is_some() {
            entry.original_bytes = original_bytes;
        }
        if converted_text.is_some() {
            entry.converted_text = converted_text;
        }
        entry.image_count = image_count;
        entry.table_count = table_count;
        Ok(())
    }

    fn get_document_content(&self, doc_id: DocId) -> Result<Option<String>> {
        let blobs = self.document_blobs.read().unwrap();
        Ok(blobs.get(&doc_id).and_then(|b| b.converted_text.clone()))
    }

    fn get_document_file(&self, doc_id: DocId) -> Result<Option<Vec<u8>>> {
        let blobs = self.document_blobs.read().unwrap();
        Ok(blobs.get(&doc_id).and_then(|b| b.original_bytes.clone()))
    }

    fn get_document_blob_stats(&self, doc_id: DocId) -> Result<(i32, i32)> {
        let blobs = self.document_blobs.read().unwrap();
        Ok(blobs
            .get(&doc_id)
            .map(|b| (b.image_count, b.table_count))
            .unwrap_or((0, 0)))
    }

    // ── User ──────────────────────────────────────────────────────────

    fn insert_user(&self, email: String, name: String, password_hash: String) -> Result<User> {
        let email_lower = email.to_lowercase();
        if self
            .user_by_email
            .read()
            .unwrap()
            .contains_key(&email_lower)
        {
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
        self.users.write().unwrap().insert(
            user.id,
            UserRecord {
                user: user.clone(),
                password_hash,
            },
        );
        self.user_by_email
            .write()
            .unwrap()
            .insert(email_lower, user.id);
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
        let existing_id = self
            .user_by_email
            .read()
            .unwrap()
            .get(&email_lower)
            .copied();

        if let Some(id) = existing_id {
            let mut users = self.users.write().unwrap();
            if let Some(record) = users.get_mut(&id) {
                record.user.name = name;
                record.user.is_super_admin = is_super_admin;
                record.user.role = role;
                record.password_hash = password_hash;
                return Ok(record.user.clone());
            }
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
        self.users.write().unwrap().insert(
            user.id,
            UserRecord {
                user: user.clone(),
                password_hash,
            },
        );
        self.user_by_email
            .write()
            .unwrap()
            .insert(email_lower, user.id);
        Ok(user)
    }

    fn delete_user(&self, id: UserId) -> Result<()> {
        let removed = self.users.write().unwrap().remove(&id);
        match removed {
            Some(record) => {
                self.user_by_email
                    .write()
                    .unwrap()
                    .remove(&record.user.email);
                Ok(())
            }
            None => Err(ThaiRagError::NotFound(format!("User {id} not found"))),
        }
    }

    fn get_user_by_email(&self, email: &str) -> Result<UserRecord> {
        let email_lower = email.to_lowercase();
        let id = self
            .user_by_email
            .read()
            .unwrap()
            .get(&email_lower)
            .copied()
            .ok_or_else(|| ThaiRagError::NotFound(format!("User with email {email} not found")))?;
        self.users
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("User {id} not found")))
    }

    fn get_user(&self, id: UserId) -> Result<User> {
        self.users
            .read()
            .unwrap()
            .get(&id)
            .map(|r| r.user.clone())
            .ok_or_else(|| ThaiRagError::NotFound(format!("User {id} not found")))
    }

    fn list_users(&self) -> Vec<User> {
        self.users
            .read()
            .unwrap()
            .values()
            .map(|r| r.user.clone())
            .collect()
    }

    // ── Identity Providers ──────────────────────────────────────────

    fn list_identity_providers(&self) -> Vec<IdentityProvider> {
        self.identity_providers
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }

    fn list_enabled_identity_providers(&self) -> Vec<IdentityProvider> {
        self.identity_providers
            .read()
            .unwrap()
            .values()
            .filter(|p| p.enabled)
            .cloned()
            .collect()
    }

    fn get_identity_provider(&self, id: IdpId) -> Result<IdentityProvider> {
        self.identity_providers
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Identity provider {id} not found")))
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
            config,
            created_at: now,
            updated_at: now,
        };
        self.identity_providers
            .write()
            .unwrap()
            .insert(idp.id, idp.clone());
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
        let mut providers = self.identity_providers.write().unwrap();
        let idp = providers
            .get_mut(&id)
            .ok_or_else(|| ThaiRagError::NotFound(format!("Identity provider {id} not found")))?;
        idp.name = name;
        idp.provider_type = provider_type;
        idp.enabled = enabled;
        idp.config = config;
        idp.updated_at = Utc::now();
        Ok(idp.clone())
    }

    fn delete_identity_provider(&self, id: IdpId) -> Result<()> {
        if self
            .identity_providers
            .write()
            .unwrap()
            .remove(&id)
            .is_none()
        {
            return Err(ThaiRagError::NotFound(format!(
                "Identity provider {id} not found"
            )));
        }
        Ok(())
    }

    // ── Permissions ──────────────────────────────────────────────────

    fn add_permission(&self, perm: UserPermission) {
        self.permissions.write().unwrap().push(perm);
    }

    fn upsert_permission(&self, perm: UserPermission) -> bool {
        let mut perms = self.permissions.write().unwrap();
        if let Some(existing) = perms
            .iter_mut()
            .find(|p| p.user_id == perm.user_id && scopes_match(&p.scope, &perm.scope))
        {
            existing.role = perm.role;
            true
        } else {
            perms.push(perm);
            false
        }
    }

    fn list_permissions_for_org(&self, org_id: OrgId) -> Vec<UserPermission> {
        self.permissions
            .read()
            .unwrap()
            .iter()
            .filter(|p| scope_org_id(&p.scope) == org_id)
            .cloned()
            .collect()
    }

    fn remove_permission(&self, user_id: UserId, scope: &PermissionScope) -> Result<()> {
        let mut perms = self.permissions.write().unwrap();
        let before = perms.len();
        perms.retain(|p| !(p.user_id == user_id && scopes_match(&p.scope, scope)));
        if perms.len() == before {
            return Err(ThaiRagError::NotFound("Permission not found".into()));
        }
        Ok(())
    }

    fn count_org_owners(&self, org_id: OrgId) -> usize {
        self.permissions
            .read()
            .unwrap()
            .iter()
            .filter(|p| {
                p.role == Role::Owner
                    && matches!(&p.scope, PermissionScope::Org { org_id: oid } if *oid == org_id)
            })
            .count()
    }

    fn get_user_role_for_org(&self, user_id: UserId, org_id: OrgId) -> Option<Role> {
        let perms = self.permissions.read().unwrap();
        perms
            .iter()
            .filter(|p| p.user_id == user_id)
            .filter(|p| match &p.scope {
                PermissionScope::Org { org_id: oid } => *oid == org_id,
                PermissionScope::Dept { org_id: oid, .. } => *oid == org_id,
                PermissionScope::Workspace { org_id: oid, .. } => *oid == org_id,
            })
            .map(|p| p.role)
            .max()
    }

    fn get_user_role_for_dept(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
    ) -> Option<Role> {
        let perms = self.permissions.read().unwrap();
        perms
            .iter()
            .filter(|p| p.user_id == user_id)
            .filter(|p| match &p.scope {
                PermissionScope::Org { org_id: oid } => *oid == org_id,
                PermissionScope::Dept {
                    org_id: oid,
                    dept_id: did,
                } => *oid == org_id && *did == dept_id,
                _ => false,
            })
            .map(|p| p.role)
            .max()
    }

    fn get_user_role_for_workspace(
        &self,
        user_id: UserId,
        org_id: OrgId,
        dept_id: DeptId,
        workspace_id: WorkspaceId,
    ) -> Option<Role> {
        let perms = self.permissions.read().unwrap();
        perms
            .iter()
            .filter(|p| p.user_id == user_id)
            .filter(|p| match &p.scope {
                PermissionScope::Org { org_id: oid } => *oid == org_id,
                PermissionScope::Dept {
                    org_id: oid,
                    dept_id: did,
                } => *oid == org_id && *did == dept_id,
                PermissionScope::Workspace {
                    org_id: oid,
                    dept_id: did,
                    workspace_id: wid,
                } => *oid == org_id && *did == dept_id && *wid == workspace_id,
            })
            .map(|p| p.role)
            .max()
    }

    fn list_user_permissions(&self, user_id: UserId) -> Vec<UserPermission> {
        self.permissions
            .read()
            .unwrap()
            .iter()
            .filter(|p| p.user_id == user_id)
            .cloned()
            .collect()
    }

    fn get_user_workspace_ids(&self, user_id: UserId) -> Vec<WorkspaceId> {
        let perms = self.permissions.read().unwrap();
        let mut ws_ids = Vec::new();
        for perm in perms.iter().filter(|p| p.user_id == user_id) {
            match &perm.scope {
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
        self.workspaces
            .read()
            .unwrap()
            .values()
            .filter(|w| w.dept_id == dept_id)
            .map(|w| w.id)
            .collect()
    }

    fn dept_ids_in_org(&self, org_id: OrgId) -> Vec<DeptId> {
        self.depts
            .read()
            .unwrap()
            .values()
            .filter(|d| d.org_id == org_id)
            .map(|d| d.id)
            .collect()
    }

    fn doc_ids_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        self.documents
            .read()
            .unwrap()
            .values()
            .filter(|d| d.workspace_id == workspace_id)
            .map(|d| d.id)
            .collect()
    }

    fn cascade_delete_workspace_docs(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        let doc_ids = self.doc_ids_in_workspace(workspace_id);
        let mut docs = self.documents.write().unwrap();
        for id in &doc_ids {
            docs.remove(id);
        }
        doc_ids
    }

    fn cascade_delete_workspace(&self, ws_id: WorkspaceId) -> Result<Vec<DocId>> {
        let doc_ids = self.cascade_delete_workspace_docs(ws_id);
        self.delete_workspace(ws_id)?;
        Ok(doc_ids)
    }

    fn cascade_delete_dept(&self, dept_id: DeptId) -> Result<Vec<DocId>> {
        let ws_ids = self.workspace_ids_in_dept(dept_id);
        let mut all_doc_ids = Vec::new();
        for ws_id in ws_ids {
            all_doc_ids.extend(self.cascade_delete_workspace_docs(ws_id));
            self.delete_workspace(ws_id)?;
        }
        self.delete_dept(dept_id)?;
        Ok(all_doc_ids)
    }

    fn cascade_delete_org(&self, org_id: OrgId) -> Result<Vec<DocId>> {
        let dept_ids = self.dept_ids_in_org(org_id);
        let mut all_doc_ids = Vec::new();
        for dept_id in dept_ids {
            let ws_ids = self.workspace_ids_in_dept(dept_id);
            for ws_id in ws_ids {
                all_doc_ids.extend(self.cascade_delete_workspace_docs(ws_id));
                self.delete_workspace(ws_id)?;
            }
            self.delete_dept(dept_id)?;
        }
        self.delete_org(org_id)?;
        Ok(all_doc_ids)
    }

    fn get_setting(&self, key: &str) -> Option<String> {
        self.settings.read().unwrap().get(key).cloned()
    }

    fn set_setting(&self, key: &str, value: &str) {
        self.settings
            .write()
            .unwrap()
            .insert(key.to_string(), value.to_string());
    }

    fn delete_setting(&self, key: &str) {
        self.settings.write().unwrap().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::models::PermissionScope;
    use thairag_core::permission::Role;

    #[test]
    fn user_crud_roundtrip() {
        let store = MemoryKmStore::new();
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
        let store = MemoryKmStore::new();
        store
            .insert_user("bob@test.com".into(), "Bob".into(), "h".into())
            .unwrap();
        let result = store.insert_user("BOB@test.com".into(), "Bob2".into(), "h2".into());
        assert!(result.is_err());
    }

    #[test]
    fn permission_resolution() {
        let store = MemoryKmStore::new();
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
    fn org_id_for_workspace_traversal() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();

        assert_eq!(store.org_id_for_workspace(ws.id).unwrap(), org.id);
    }

    #[test]
    fn no_permission_returns_none() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let user = store
            .insert_user("u@t.com".into(), "U".into(), "h".into())
            .unwrap();
        assert_eq!(store.get_user_role_for_org(user.id, org.id), None);
    }

    #[test]
    fn org_crud_roundtrip() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme Corp".into()).unwrap();
        assert_eq!(store.get_org(org.id).unwrap().name, "Acme Corp");
        assert_eq!(store.list_orgs().len(), 1);
        store.delete_org(org.id).unwrap();
        assert!(store.get_org(org.id).is_err());
    }

    #[test]
    fn dept_requires_valid_org() {
        let store = MemoryKmStore::new();
        let result = store.insert_dept(OrgId::new(), "Engineering".into());
        assert!(result.is_err());
    }

    #[test]
    fn dept_crud_roundtrip() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Engineering".into()).unwrap();
        assert_eq!(store.get_dept(dept.id).unwrap().name, "Engineering");
        assert_eq!(store.list_depts_in_org(org.id).len(), 1);
        store.delete_dept(dept.id).unwrap();
        assert!(store.get_dept(dept.id).is_err());
    }

    #[test]
    fn workspace_requires_valid_dept() {
        let store = MemoryKmStore::new();
        let result = store.insert_workspace(DeptId::new(), "ws".into());
        assert!(result.is_err());
    }

    #[test]
    fn workspace_crud_roundtrip() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let ws = store.insert_workspace(dept.id, "Main".into()).unwrap();
        assert_eq!(store.get_workspace(ws.id).unwrap().name, "Main");
        assert_eq!(store.list_workspaces_in_dept(dept.id).len(), 1);
        store.delete_workspace(ws.id).unwrap();
        assert!(store.get_workspace(ws.id).is_err());
    }

    #[test]
    fn document_requires_valid_workspace() {
        let store = MemoryKmStore::new();
        let now = Utc::now();
        let doc = Document {
            id: DocId::new(),
            workspace_id: WorkspaceId::new(),
            title: "test".into(),
            mime_type: "text/plain".into(),
            size_bytes: 0,
            status: DocStatus::Ready,
            chunk_count: 0,
            error_message: None,
            processing_step: None,
            created_at: now,
            updated_at: now,
        };
        assert!(store.insert_document(doc).is_err());
    }

    #[test]
    fn document_crud_roundtrip() {
        let store = MemoryKmStore::new();
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
    fn not_found_errors() {
        let store = MemoryKmStore::new();
        assert!(store.get_org(OrgId::new()).is_err());
        assert!(store.delete_org(OrgId::new()).is_err());
        assert!(store.get_dept(DeptId::new()).is_err());
        assert!(store.delete_dept(DeptId::new()).is_err());
        assert!(store.get_workspace(WorkspaceId::new()).is_err());
        assert!(store.delete_workspace(WorkspaceId::new()).is_err());
        assert!(store.get_document(DocId::new()).is_err());
        assert!(store.delete_document(DocId::new()).is_err());
    }

    #[test]
    fn cascade_delete_org() {
        let store = MemoryKmStore::new();
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

        let deleted_doc_ids = store.cascade_delete_org(org.id).unwrap();
        assert_eq!(deleted_doc_ids.len(), 1);
        assert_eq!(deleted_doc_ids[0], doc.id);
        assert!(store.get_org(org.id).is_err());
        assert!(store.get_dept(dept.id).is_err());
        assert!(store.get_workspace(ws.id).is_err());
        assert!(store.get_document(doc.id).is_err());
    }

    #[test]
    fn cascade_delete_dept() {
        let store = MemoryKmStore::new();
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
        store.insert_document(doc).unwrap();

        let deleted = store.cascade_delete_dept(dept.id).unwrap();
        assert_eq!(deleted.len(), 1);
        assert!(store.get_org(org.id).is_ok());
        assert!(store.get_dept(dept.id).is_err());
        assert!(store.get_workspace(ws.id).is_err());
    }

    #[test]
    fn upsert_permission_dedup() {
        let store = MemoryKmStore::new();
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
    fn list_permissions_filters_by_org() {
        let store = MemoryKmStore::new();
        let org_a = store.insert_org("A".into()).unwrap();
        let org_b = store.insert_org("B".into()).unwrap();
        let user = store
            .insert_user("u@t.com".into(), "U".into(), "h".into())
            .unwrap();

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Org { org_id: org_a.id },
            role: Role::Owner,
        });
        store.add_permission(UserPermission {
            user_id: user.id,
            scope: PermissionScope::Org { org_id: org_b.id },
            role: Role::Viewer,
        });

        assert_eq!(store.list_permissions_for_org(org_a.id).len(), 1);
        assert_eq!(store.list_permissions_for_org(org_b.id).len(), 1);
    }

    #[test]
    fn remove_permission_success_and_not_found() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let user = store
            .insert_user("u@t.com".into(), "U".into(), "h".into())
            .unwrap();
        let scope = PermissionScope::Org { org_id: org.id };

        store.add_permission(UserPermission {
            user_id: user.id,
            scope: scope.clone(),
            role: Role::Viewer,
        });

        store.remove_permission(user.id, &scope).unwrap();
        assert_eq!(store.list_permissions_for_org(org.id).len(), 0);

        assert!(store.remove_permission(user.id, &scope).is_err());
    }

    #[test]
    fn count_org_owners_counts_correctly() {
        let store = MemoryKmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Eng".into()).unwrap();
        let u1 = store
            .insert_user("a@t.com".into(), "A".into(), "h".into())
            .unwrap();
        let u2 = store
            .insert_user("b@t.com".into(), "B".into(), "h".into())
            .unwrap();

        store.add_permission(UserPermission {
            user_id: u1.id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Owner,
        });
        store.add_permission(UserPermission {
            user_id: u2.id,
            scope: PermissionScope::Org { org_id: org.id },
            role: Role::Admin,
        });
        store.add_permission(UserPermission {
            user_id: u2.id,
            scope: PermissionScope::Dept {
                org_id: org.id,
                dept_id: dept.id,
            },
            role: Role::Owner,
        });

        assert_eq!(store.count_org_owners(org.id), 1);
    }

    #[test]
    fn cascade_delete_workspace() {
        let store = MemoryKmStore::new();
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
        store.insert_document(doc).unwrap();

        let deleted = store.cascade_delete_workspace(ws.id).unwrap();
        assert_eq!(deleted.len(), 1);
        assert!(store.get_org(org.id).is_ok());
        assert!(store.get_dept(dept.id).is_ok());
        assert!(store.get_workspace(ws.id).is_err());
    }
}
