use serde::{Deserialize, Serialize};

use crate::types::WorkspaceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Editor,
    Admin,
    Owner,
}

impl Role {
    pub fn can_read(&self) -> bool {
        true
    }

    pub fn can_write(&self) -> bool {
        matches!(self, Role::Editor | Role::Admin | Role::Owner)
    }

    pub fn can_manage(&self) -> bool {
        matches!(self, Role::Admin | Role::Owner)
    }

    pub fn can_delete(&self) -> bool {
        matches!(self, Role::Owner)
    }
}

#[derive(Debug, Clone)]
pub struct AccessScope {
    pub workspace_ids: Vec<WorkspaceId>,
}

impl AccessScope {
    pub fn new(workspace_ids: Vec<WorkspaceId>) -> Self {
        Self { workspace_ids }
    }

    pub fn unrestricted() -> Self {
        Self {
            workspace_ids: vec![],
        }
    }

    pub fn is_unrestricted(&self) -> bool {
        self.workspace_ids.is_empty()
    }
}
