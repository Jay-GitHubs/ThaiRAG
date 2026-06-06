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

/// Record of a single AI agent's participation in processing a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    /// Agent identifier: "analyzer", "chunker", "enricher", "converter", "quality".
    pub agent: String,
    /// Model that backed this agent, when applicable (e.g. "qwen2.5:7b").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Outcome: "ran", "skipped", "failed".
    pub status: String,
    /// Optional human-readable reason (e.g. "skipped, vision").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Timing record for a single processing stage. The pipeline reports a stream
/// of step names (analyzing, converting, chunking, indexing, …); each distinct
/// step becomes one `StageTiming`. `duration_ms` is `None` while the stage is
/// still in progress and filled in when the next step starts (or processing
/// terminates). Powers the admin UI's live per-stage tracker + bottleneck view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTiming {
    /// Raw step identifier as reported by the pipeline (e.g. "converting").
    pub step: String,
    /// Wall-clock start of this stage, epoch milliseconds.
    pub started_at_ms: i64,
    /// Elapsed time for this stage in milliseconds, or `None` if still running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Model backing this stage (e.g. an LLM agent's model), when applicable.
    /// Recorded live as the step starts so operators can see — and abort on —
    /// the wrong model mid-run. `None` for non-model stages (e.g. indexing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl StageTiming {
    /// Advance a processing timeline by one step transition.
    ///
    /// Closes the currently-open stage (filling its `duration_ms`) and, when
    /// `step` is `Some`, opens a new stage starting at `now_ms`. Passing `None`
    /// closes the timeline at a terminal state (ready/failed). Consecutive
    /// reports of the same step name are coalesced — the open stage keeps
    /// running — so retries that re-report a step don't create duplicates.
    pub fn advance(
        timeline: &mut Vec<StageTiming>,
        step: Option<&str>,
        now_ms: i64,
        model: Option<&str>,
    ) {
        let same_as_open = matches!(
            (timeline.last(), step),
            (Some(last), Some(s)) if last.step == s && last.duration_ms.is_none()
        );
        if same_as_open {
            return;
        }
        if let Some(last) = timeline.last_mut()
            && last.duration_ms.is_none()
        {
            last.duration_ms = Some((now_ms - last.started_at_ms).max(0));
        }
        if let Some(s) = step {
            timeline.push(StageTiming {
                step: s.to_string(),
                started_at_ms: now_ms,
                duration_ms: None,
                model: model.map(|m| m.to_string()),
            });
        }
    }
}

/// Persistent, per-document summary of how a document was processed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingProvenance {
    /// Processing path label (e.g. "smart-PDF + AI agents", "embedded-media + AI agents",
    /// "direct-image", "AI agents", "mechanical").
    pub path: String,
    /// Per-agent participation records.
    #[serde(default)]
    pub agents: Vec<AgentRun>,
    /// Whether AI processing fell back to mechanical chunking.
    #[serde(default)]
    pub mechanical_fallback: bool,
    /// Final chunk count produced.
    #[serde(default)]
    pub chunk_count: i64,
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
    /// Persistent record of how this document was processed (path, agents, models, fallback).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_provenance: Option<ProcessingProvenance>,
    /// Per-stage timing accumulated during processing; drives the admin UI's
    /// live step tracker and bottleneck breakdown. Reset on each (re)processing run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing_timeline: Option<Vec<StageTiming>>,
    /// Current version number (1-indexed, increments on each update).
    #[serde(default = "default_version")]
    pub version: i32,
    /// SHA-256 hash of the document content for change detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// URL the document was fetched from (for scheduled re-ingestion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// Refresh interval (e.g., "1h", "6h", "1d", "7d", "30d").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_schedule: Option<String>,
    /// Timestamp of last successful refresh from source_url.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_version() -> i32 {
    1
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
    #[serde(default)]
    pub disabled: bool,
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
    Org {
        org_id: OrgId,
    },
    Dept {
        org_id: OrgId,
        dept_id: DeptId,
    },
    Workspace {
        org_id: OrgId,
        dept_id: DeptId,
        workspace_id: WorkspaceId,
    },
}

#[cfg(test)]
mod stage_timing_tests {
    use super::StageTiming;

    #[test]
    fn advance_closes_prior_and_opens_new() {
        let mut tl = Vec::new();
        StageTiming::advance(&mut tl, Some("converting"), 1_000, Some("qwen"));
        StageTiming::advance(&mut tl, Some("chunking"), 1_400, Some("qwen"));
        assert_eq!(tl.len(), 2);
        // First stage closed with its measured duration; second still running.
        assert_eq!(tl[0].step, "converting");
        assert_eq!(tl[0].duration_ms, Some(400));
        assert_eq!(tl[0].model.as_deref(), Some("qwen"));
        assert_eq!(tl[1].step, "chunking");
        assert_eq!(tl[1].duration_ms, None);
    }

    #[test]
    fn advance_none_closes_final_stage() {
        let mut tl = Vec::new();
        StageTiming::advance(&mut tl, Some("indexing"), 2_000, None);
        StageTiming::advance(&mut tl, None, 2_500, None);
        assert_eq!(tl.len(), 1);
        assert_eq!(tl[0].duration_ms, Some(500));
        assert_eq!(tl[0].model, None);
    }

    #[test]
    fn advance_coalesces_repeated_step() {
        let mut tl = Vec::new();
        StageTiming::advance(&mut tl, Some("indexing"), 100, None);
        StageTiming::advance(&mut tl, Some("indexing"), 300, None);
        // Same step reported twice: keep one open stage rather than duplicating.
        assert_eq!(tl.len(), 1);
        assert_eq!(tl[0].started_at_ms, 100);
        assert_eq!(tl[0].duration_ms, None);
    }

    #[test]
    fn advance_clamps_negative_duration_to_zero() {
        let mut tl = Vec::new();
        StageTiming::advance(&mut tl, Some("a"), 1_000, None);
        // A clock that goes backwards must not yield a negative duration.
        StageTiming::advance(&mut tl, Some("b"), 500, None);
        assert_eq!(tl[0].duration_ms, Some(0));
    }
}
