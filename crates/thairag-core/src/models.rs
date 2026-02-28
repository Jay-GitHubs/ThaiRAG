use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::permission::Role;
use crate::types::{DeptId, DocId, OrgId, UserId, WorkspaceId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: OrgId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Department {
    pub id: DeptId,
    pub org_id: OrgId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub dept_id: DeptId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermission {
    pub user_id: UserId,
    pub scope: PermissionScope,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "level")]
pub enum PermissionScope {
    Org { org_id: OrgId },
    Dept { org_id: OrgId, dept_id: DeptId },
    Workspace { org_id: OrgId, dept_id: DeptId, workspace_id: WorkspaceId },
}
