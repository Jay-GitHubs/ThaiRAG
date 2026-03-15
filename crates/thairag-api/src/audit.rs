use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::store::KmStoreTrait;

/// An audit log entry for security-sensitive operations (OWASP A09).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    /// Who performed the action (user ID or "system").
    pub actor: String,
    /// What action was performed.
    pub action: AuditAction,
    /// Target entity (e.g., user email, org name, permission scope).
    pub target: String,
    /// Whether the action succeeded.
    pub success: bool,
    /// Optional details (e.g., role granted, error reason).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Login,
    LoginFailed,
    Register,
    UserDeleted,
    PermissionGranted,
    PermissionRevoked,
    SettingsChanged,
    IdpCreated,
    IdpUpdated,
    IdpDeleted,
    PromptUpdated,
    PromptDeleted,
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{self:?}"));
        write!(f, "{s}")
    }
}

/// Append an audit entry to the KV-based audit log.
pub fn audit_log(
    store: &Arc<dyn KmStoreTrait>,
    actor: &str,
    action: AuditAction,
    target: &str,
    success: bool,
    detail: Option<&str>,
) {
    let entry = AuditEntry {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        actor: actor.to_string(),
        action: action.clone(),
        target: target.to_string(),
        success,
        detail: detail.map(|s| s.to_string()),
    };

    // Store as a JSON array under the "audit_log" setting key.
    // Keep last 1000 entries to prevent unbounded growth.
    let key = "audit_log";
    let mut entries: Vec<AuditEntry> = store
        .get_setting(key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    entries.push(entry.clone());

    // Trim to last 1000 entries
    if entries.len() > 1000 {
        entries.drain(..entries.len() - 1000);
    }

    if let Ok(json) = serde_json::to_string(&entries) {
        store.set_setting(key, &json);
    }

    // Also emit structured tracing for log aggregation
    tracing::info!(
        audit_action = %action,
        actor = %entry.actor,
        target = %entry.target,
        success = entry.success,
        detail = entry.detail.as_deref().unwrap_or(""),
        "AUDIT"
    );
}

/// Query audit log entries, with optional filtering.
pub fn get_audit_log(
    store: &Arc<dyn KmStoreTrait>,
    action_filter: Option<&str>,
    limit: usize,
) -> Vec<AuditEntry> {
    let key = "audit_log";
    let mut entries: Vec<AuditEntry> = store
        .get_setting(key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    if let Some(filter) = action_filter {
        entries.retain(|e| {
            let action_str = e.action.to_string();
            action_str == filter
        });
    }

    // Return most recent first
    entries.reverse();
    entries.truncate(limit);
    entries
}
