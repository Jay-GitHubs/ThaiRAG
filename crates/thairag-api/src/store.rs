use std::collections::HashMap;
use std::sync::RwLock;

use chrono::Utc;
use thairag_core::models::{Department, Document, Organization, Workspace};
use thairag_core::types::{DeptId, DocId, OrgId, WorkspaceId};
use thairag_core::ThaiRagError;

type Result<T> = std::result::Result<T, ThaiRagError>;

pub struct KmStore {
    orgs: RwLock<HashMap<OrgId, Organization>>,
    depts: RwLock<HashMap<DeptId, Department>>,
    workspaces: RwLock<HashMap<WorkspaceId, Workspace>>,
    documents: RwLock<HashMap<DocId, Document>>,
}

impl KmStore {
    pub fn new() -> Self {
        Self {
            orgs: RwLock::new(HashMap::new()),
            depts: RwLock::new(HashMap::new()),
            workspaces: RwLock::new(HashMap::new()),
            documents: RwLock::new(HashMap::new()),
        }
    }

    // ── Organization ────────────────────────────────────────────────

    pub fn insert_org(&self, name: String) -> Result<Organization> {
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

    pub fn get_org(&self, id: OrgId) -> Result<Organization> {
        self.orgs
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Organization {id} not found")))
    }

    pub fn list_orgs(&self) -> Vec<Organization> {
        self.orgs.read().unwrap().values().cloned().collect()
    }

    pub fn delete_org(&self, id: OrgId) -> Result<()> {
        if self.orgs.write().unwrap().remove(&id).is_none() {
            return Err(ThaiRagError::NotFound(format!("Organization {id} not found")));
        }
        Ok(())
    }

    // ── Department ──────────────────────────────────────────────────

    pub fn insert_dept(&self, org_id: OrgId, name: String) -> Result<Department> {
        // Validate parent exists
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

    pub fn get_dept(&self, id: DeptId) -> Result<Department> {
        self.depts
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Department {id} not found")))
    }

    pub fn list_depts_in_org(&self, org_id: OrgId) -> Vec<Department> {
        self.depts
            .read()
            .unwrap()
            .values()
            .filter(|d| d.org_id == org_id)
            .cloned()
            .collect()
    }

    pub fn delete_dept(&self, id: DeptId) -> Result<()> {
        if self.depts.write().unwrap().remove(&id).is_none() {
            return Err(ThaiRagError::NotFound(format!("Department {id} not found")));
        }
        Ok(())
    }

    // ── Workspace ───────────────────────────────────────────────────

    pub fn insert_workspace(&self, dept_id: DeptId, name: String) -> Result<Workspace> {
        // Validate parent exists
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

    pub fn get_workspace(&self, id: WorkspaceId) -> Result<Workspace> {
        self.workspaces
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Workspace {id} not found")))
    }

    pub fn list_workspaces_in_dept(&self, dept_id: DeptId) -> Vec<Workspace> {
        self.workspaces
            .read()
            .unwrap()
            .values()
            .filter(|w| w.dept_id == dept_id)
            .cloned()
            .collect()
    }

    pub fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
        if self.workspaces.write().unwrap().remove(&id).is_none() {
            return Err(ThaiRagError::NotFound(format!("Workspace {id} not found")));
        }
        Ok(())
    }

    // ── Document ────────────────────────────────────────────────────

    pub fn insert_document(&self, doc: Document) -> Result<Document> {
        // Validate parent exists
        self.get_workspace(doc.workspace_id)?;
        self.documents.write().unwrap().insert(doc.id, doc.clone());
        Ok(doc)
    }

    pub fn get_document(&self, id: DocId) -> Result<Document> {
        self.documents
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| ThaiRagError::NotFound(format!("Document {id} not found")))
    }

    pub fn list_documents_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<Document> {
        self.documents
            .read()
            .unwrap()
            .values()
            .filter(|d| d.workspace_id == workspace_id)
            .cloned()
            .collect()
    }

    pub fn delete_document(&self, id: DocId) -> Result<()> {
        if self.documents.write().unwrap().remove(&id).is_none() {
            return Err(ThaiRagError::NotFound(format!("Document {id} not found")));
        }
        Ok(())
    }

    // ── Cascade helpers ─────────────────────────────────────────────

    /// Collect all workspace IDs belonging to a department.
    pub fn workspace_ids_in_dept(&self, dept_id: DeptId) -> Vec<WorkspaceId> {
        self.workspaces
            .read()
            .unwrap()
            .values()
            .filter(|w| w.dept_id == dept_id)
            .map(|w| w.id)
            .collect()
    }

    /// Collect all department IDs belonging to an organization.
    pub fn dept_ids_in_org(&self, org_id: OrgId) -> Vec<DeptId> {
        self.depts
            .read()
            .unwrap()
            .values()
            .filter(|d| d.org_id == org_id)
            .map(|d| d.id)
            .collect()
    }

    /// Collect all document IDs belonging to a workspace.
    pub fn doc_ids_in_workspace(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        self.documents
            .read()
            .unwrap()
            .values()
            .filter(|d| d.workspace_id == workspace_id)
            .map(|d| d.id)
            .collect()
    }

    /// Cascade delete: remove all documents in a workspace.
    pub fn cascade_delete_workspace_docs(&self, workspace_id: WorkspaceId) -> Vec<DocId> {
        let doc_ids = self.doc_ids_in_workspace(workspace_id);
        let mut docs = self.documents.write().unwrap();
        for id in &doc_ids {
            docs.remove(id);
        }
        doc_ids
    }

    /// Cascade delete: remove a workspace and its documents, returning affected doc IDs.
    pub fn cascade_delete_workspace(&self, ws_id: WorkspaceId) -> Result<Vec<DocId>> {
        let doc_ids = self.cascade_delete_workspace_docs(ws_id);
        self.delete_workspace(ws_id)?;
        Ok(doc_ids)
    }

    /// Cascade delete: remove a department, its workspaces, and all nested documents.
    pub fn cascade_delete_dept(&self, dept_id: DeptId) -> Result<Vec<DocId>> {
        let ws_ids = self.workspace_ids_in_dept(dept_id);
        let mut all_doc_ids = Vec::new();
        for ws_id in ws_ids {
            all_doc_ids.extend(self.cascade_delete_workspace_docs(ws_id));
            self.delete_workspace(ws_id)?;
        }
        self.delete_dept(dept_id)?;
        Ok(all_doc_ids)
    }

    /// Cascade delete: remove an org, its departments, workspaces, and all documents.
    pub fn cascade_delete_org(&self, org_id: OrgId) -> Result<Vec<DocId>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_crud_roundtrip() {
        let store = KmStore::new();
        let org = store.insert_org("Acme Corp".into()).unwrap();
        assert_eq!(store.get_org(org.id).unwrap().name, "Acme Corp");
        assert_eq!(store.list_orgs().len(), 1);
        store.delete_org(org.id).unwrap();
        assert!(store.get_org(org.id).is_err());
    }

    #[test]
    fn dept_requires_valid_org() {
        let store = KmStore::new();
        let result = store.insert_dept(OrgId::new(), "Engineering".into());
        assert!(result.is_err());
    }

    #[test]
    fn dept_crud_roundtrip() {
        let store = KmStore::new();
        let org = store.insert_org("Acme".into()).unwrap();
        let dept = store.insert_dept(org.id, "Engineering".into()).unwrap();
        assert_eq!(store.get_dept(dept.id).unwrap().name, "Engineering");
        assert_eq!(store.list_depts_in_org(org.id).len(), 1);
        store.delete_dept(dept.id).unwrap();
        assert!(store.get_dept(dept.id).is_err());
    }

    #[test]
    fn workspace_requires_valid_dept() {
        let store = KmStore::new();
        let result = store.insert_workspace(DeptId::new(), "ws".into());
        assert!(result.is_err());
    }

    #[test]
    fn workspace_crud_roundtrip() {
        let store = KmStore::new();
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
        let store = KmStore::new();
        let now = Utc::now();
        let doc = Document {
            id: DocId::new(),
            workspace_id: WorkspaceId::new(),
            title: "test".into(),
            mime_type: "text/plain".into(),
            size_bytes: 0,
            created_at: now,
            updated_at: now,
        };
        assert!(store.insert_document(doc).is_err());
    }

    #[test]
    fn document_crud_roundtrip() {
        let store = KmStore::new();
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
    fn not_found_errors() {
        let store = KmStore::new();
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
        let store = KmStore::new();
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
        let store = KmStore::new();
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
        store.insert_document(doc).unwrap();

        let deleted = store.cascade_delete_dept(dept.id).unwrap();
        assert_eq!(deleted.len(), 1);
        // Org should still exist
        assert!(store.get_org(org.id).is_ok());
        assert!(store.get_dept(dept.id).is_err());
        assert!(store.get_workspace(ws.id).is_err());
    }

    #[test]
    fn cascade_delete_workspace() {
        let store = KmStore::new();
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
        store.insert_document(doc).unwrap();

        let deleted = store.cascade_delete_workspace(ws.id).unwrap();
        assert_eq!(deleted.len(), 1);
        // Org and dept should still exist
        assert!(store.get_org(org.id).is_ok());
        assert!(store.get_dept(dept.id).is_ok());
        assert!(store.get_workspace(ws.id).is_err());
    }
}
