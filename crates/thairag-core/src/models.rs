use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::permission::Role;
use crate::types::{DeptId, DocId, IdpId, OrgId, UserId, WorkspaceId};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStatus {
    Processing,
    Ready,
    Failed,
}

impl std::fmt::Display for DocStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Processing => write!(f, "processing"),
            Self::Ready => write!(f, "ready"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl DocStatus {
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "processing" => Self::Processing,
            "failed" => Self::Failed,
            _ => Self::Ready,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub mime_type: String,
    pub size_bytes: i64,
    #[serde(default = "default_doc_status")]
    pub status: DocStatus,
    #[serde(default)]
    pub chunk_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Current AI preprocessing step (analyzing, converting, checking_quality, chunking, indexing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_step: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_doc_status() -> DocStatus {
    DocStatus::Ready
}

fn default_local() -> String {
    "local".to_string()
}

fn default_viewer() -> String {
    "viewer".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: String,
    #[serde(default = "default_local")]
    pub auth_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default)]
    pub is_super_admin: bool,
    #[serde(default = "default_viewer")]
    pub role: String,
    pub created_at: DateTime<Utc>,
}

impl User {
    /// Ensure role is consistent with is_super_admin flag.
    /// Call after loading from DB to handle legacy data.
    pub fn normalize_role(mut self) -> Self {
        if self.is_super_admin && self.role != "super_admin" {
            self.role = "super_admin".to_string();
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityProvider {
    pub id: IdpId,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
