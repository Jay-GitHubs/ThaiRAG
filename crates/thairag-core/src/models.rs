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

/// Deterministic conversion-fidelity assessment: how faithfully the converted
/// text (what feeds the vector DB) preserves the original document's extractable
/// content. Computed at ingest by comparing token sets (no LLM), so it cannot
/// itself hallucinate. `status` is "verified" (nothing dropped/fabricated),
/// "review" (numbers dropped or fabricated, or low coverage), or "unverifiable"
/// (the original has no extractable text layer — e.g. a scanned PDF).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionFidelity {
    /// "verified" | "review" | "unverifiable".
    pub status: String,
    /// Overall fidelity score in [0,1] (1.0 = full coverage, no fabrication).
    pub score: f32,
    /// Distinct numeric tokens found in the original (Thai digits normalised).
    pub numbers_total: usize,
    /// Of those, how many appear in the converted text.
    pub numbers_matched: usize,
    /// Numeric tokens in the converted text that are absent from the original
    /// (a fabrication signal — should be 0 on the deterministic paths).
    pub numbers_fabricated: usize,
    /// Fraction of the original's non-space characters present in the output.
    pub char_coverage: f32,
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
    /// Conversion-fidelity assessment (converted text vs original). Populated at
    /// ingest; `None` for older documents processed before this existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fidelity: Option<ConversionFidelity>,
    /// Count of PDF pages classified as tabular whose table could not be
    /// reconstructed deterministically; their raw text was kept verbatim
    /// (numbers exact, structure not recovered) rather than risking vision OCR.
    /// A non-zero value flags pages an analyst may want to review by hand.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tables_kept_as_text: i64,
    /// Distinguishing facets extracted at ingest ("key: value" strings, e.g.
    /// "program: SME กล้าสู้", "collateral: เงินฝาก", "limit: 10 ล้านบาท"). Used by
    /// deterministic document selection to pick the right document among
    /// near-identical siblings. Empty unless facet extraction was enabled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<String>,
    /// Which extraction engines actually ran (deterministic OCR vs vision LLM),
    /// so an operator can see — per document, after the fact — whether e.g.
    /// PaddleOCR transcribed any pages. Defaulted (all-zero) for non-PDF paths
    /// and documents processed before this was recorded.
    #[serde(default, skip_serializing_if = "ExtractionStats::is_empty")]
    pub extraction: ExtractionStats,
}

/// Per-document record of which extraction engines ran during smart-PDF
/// processing. A page is transcribed by exactly one of: the native text layer,
/// the deterministic OCR tier (PaddleOCR sidecar), or the vision LLM.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionStats {
    /// Total pages in the source PDF (0 for non-PDF paths).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub total_pages: i64,
    /// Pages transcribed by the deterministic OCR tier.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ocr_pages_used: i64,
    /// Name of the OCR provider that ran (e.g. "paddleocr-sidecar"); `None` if
    /// no page was OCR'd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_provider: Option<String>,
    /// Pages transcribed/described by the vision LLM.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub vision_pages_used: i64,
    /// Name of the vision model that ran; `None` if no page used vision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision_model: Option<String>,
    /// Pages that needed OCR/vision but had no model configured, so their raw
    /// text layer was kept. A non-zero value flags an under-configured pipeline.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pages_vision_skipped: i64,
}

impl ExtractionStats {
    /// True when nothing engine-specific was recorded (the all-default state),
    /// so it can be omitted from serialized provenance.
    pub fn is_empty(&self) -> bool {
        self.total_pages == 0
            && self.ocr_pages_used == 0
            && self.ocr_provider.is_none()
            && self.vision_pages_used == 0
            && self.vision_model.is_none()
            && self.pages_vision_skipped == 0
    }
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// A reasoning-based ("PageIndex") table-of-contents tree for a document.
///
/// Built once at ingest (or via explicit backfill) by an LLM reading the whole
/// converted text, then persisted as JSON. At query time, the reasoning-based
/// retrieval mode hands the LLM this tree's node titles + summaries and asks it
/// to *navigate* to the relevant nodes instead of relying on vector/lexical
/// similarity — a better fit for structured/tabular Thai docs and near-clone
/// corpora where the right section can be reasoned about from its summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTree {
    /// The document this tree describes.
    pub doc_id: DocId,
    /// Document title (denormalized for self-contained navigation outlines).
    pub title: String,
    /// Root node — its `children` are the document's top-level sections. The
    /// root itself carries the document-level summary used for coarse,
    /// cross-document selection before drilling in.
    pub root: DocTreeNode,
    /// Model that built the tree (provenance; pure-LLM trees are
    /// non-deterministic across rebuilds, so we record which model produced this one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

/// One node in a [`DocTree`] — a section/subsection of the document.
///
/// `node_id`s are assigned deterministically by the tree builder (a stable
/// path-like id, e.g. `"n0"`, `"n0.1"`), not taken from the LLM, so navigation
/// can reference nodes unambiguously and content can be re-resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTreeNode {
    /// Stable, builder-assigned id (path-like, e.g. `"n0.1.2"`).
    pub node_id: String,
    /// Section heading / title.
    pub title: String,
    /// One-to-two sentence summary of what this section covers, used to drive
    /// LLM navigation without sending the full section text.
    pub summary: String,
    /// 1-indexed page the section starts on, when derivable (PDFs). Used to map
    /// a selected node back to its chunks via `ChunkMetadata.page_numbers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_start: Option<usize>,
    /// 1-indexed page the section ends on (inclusive), when derivable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_end: Option<usize>,
    /// Child sections, in document order.
    #[serde(default)]
    pub children: Vec<DocTreeNode>,
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
mod extraction_stats_tests {
    use super::{ExtractionStats, ProcessingProvenance};

    // Back-compat: provenance JSON written before `extraction` existed must still
    // deserialize, defaulting to an empty (omitted) ExtractionStats.
    #[test]
    fn legacy_provenance_without_extraction_deserializes() {
        let json = r#"{"path":"smart-PDF (mechanical)","agents":[],"mechanical_fallback":false,"chunk_count":12}"#;
        let prov: ProcessingProvenance = serde_json::from_str(json).unwrap();
        assert_eq!(prov.path, "smart-PDF (mechanical)");
        assert!(
            prov.extraction.is_empty(),
            "missing extraction → empty default"
        );
    }

    // An empty ExtractionStats is omitted from serialized provenance (skip_if),
    // so non-PDF paths don't carry a noisy all-zero block.
    #[test]
    fn empty_extraction_is_skipped_in_serialization() {
        let prov = ProcessingProvenance {
            path: "embedded-media (mechanical)".into(),
            agents: vec![],
            mechanical_fallback: false,
            chunk_count: 3,
            fidelity: None,
            tables_kept_as_text: 0,
            facets: vec![],
            extraction: ExtractionStats::default(),
        };
        let json = serde_json::to_string(&prov).unwrap();
        assert!(
            !json.contains("extraction"),
            "empty extraction must be omitted: {json}"
        );
    }

    // A populated ExtractionStats round-trips and surfaces the OCR provider —
    // the signal the UI shows ("OCR 46 (paddleocr-sidecar)").
    #[test]
    fn populated_extraction_round_trips_with_provider() {
        let prov = ProcessingProvenance {
            path: "smart-PDF + AI agents".into(),
            agents: vec![],
            mechanical_fallback: false,
            chunk_count: 50,
            fidelity: None,
            tables_kept_as_text: 0,
            facets: vec![],
            extraction: ExtractionStats {
                total_pages: 46,
                ocr_pages_used: 46,
                ocr_provider: Some("paddleocr-sidecar".into()),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&prov).unwrap();
        assert!(json.contains("paddleocr-sidecar"));
        let back: ProcessingProvenance = serde_json::from_str(&json).unwrap();
        assert_eq!(back.extraction.ocr_pages_used, 46);
        assert_eq!(
            back.extraction.ocr_provider.as_deref(),
            Some("paddleocr-sidecar")
        );
        assert!(!back.extraction.is_empty());
    }
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

#[cfg(test)]
mod doc_tree_tests {
    use super::{DocTree, DocTreeNode};
    use crate::types::DocId;

    fn sample_tree() -> DocTree {
        DocTree {
            doc_id: DocId(uuid::Uuid::nil()),
            title: "สินเชื่อ SME".to_string(),
            root: DocTreeNode {
                node_id: "n0".to_string(),
                title: "สินเชื่อ SME".to_string(),
                summary: "Document covering SME loan terms.".to_string(),
                page_start: Some(1),
                page_end: Some(4),
                children: vec![DocTreeNode {
                    node_id: "n0.0".to_string(),
                    title: "วงเงิน".to_string(),
                    summary: "Credit limit and collateral.".to_string(),
                    page_start: Some(2),
                    page_end: Some(2),
                    children: vec![],
                }],
            },
            model_name: Some("qwen3:14b".to_string()),
        }
    }

    #[test]
    fn round_trips_through_json() {
        let tree = sample_tree();
        let json = serde_json::to_string(&tree).unwrap();
        let back: DocTree = serde_json::from_str(&json).unwrap();
        assert_eq!(back.doc_id, tree.doc_id);
        assert_eq!(back.title, tree.title);
        assert_eq!(back.root.children.len(), 1);
        assert_eq!(back.root.children[0].node_id, "n0.0");
        assert_eq!(back.root.children[0].page_start, Some(2));
        assert_eq!(back.model_name.as_deref(), Some("qwen3:14b"));
    }

    #[test]
    fn optional_fields_default_when_absent() {
        // A minimal node with no pages / no children and a tree with no model.
        let json = r#"{
            "doc_id": "00000000-0000-0000-0000-000000000000",
            "title": "t",
            "root": {"node_id": "n0", "title": "t", "summary": "s"}
        }"#;
        let tree: DocTree = serde_json::from_str(json).unwrap();
        assert!(tree.model_name.is_none());
        assert!(tree.root.page_start.is_none());
        assert!(tree.root.children.is_empty());
    }
}
