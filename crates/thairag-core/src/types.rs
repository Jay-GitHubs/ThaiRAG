use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

// ── ID Newtypes ──────────────────────────────────────────────────────

macro_rules! define_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(OrgId);
define_id!(DeptId);
define_id!(WorkspaceId);
define_id!(DocId);
define_id!(ChunkId);
define_id!(UserId);
define_id!(SessionId);
define_id!(IdpId);
define_id!(MemoryId);
define_id!(ConnectorId);
define_id!(SyncRunId);
define_id!(JobId);
define_id!(ApiKeyId);
define_id!(WebhookId);

// ── Provider Kind Enums ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LlmKind {
    Ollama,
    Claude,
    OpenAi,
    OpenAiCompatible,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingKind {
    Fastembed,
    OpenAi,
    Ollama,
    Cohere,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VectorStoreKind {
    InMemory,
    Qdrant,
    Pgvector,
    ChromaDb,
    Pinecone,
    Weaviate,
    Milvus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorIsolation {
    /// Single collection with metadata filtering (default).
    #[default]
    Shared,
    /// Separate collection per organization.
    PerOrganization,
    /// Separate collection per workspace.
    PerWorkspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextSearchKind {
    Tantivy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RerankerKind {
    Passthrough,
    Cohere,
    Jina,
}

// ── LLM Response Types ──────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub usage: LlmUsage,
}

pub struct LlmStreamResponse {
    pub stream: Pin<Box<dyn Stream<Item = Result<String>> + Send>>,
    pub usage: Arc<Mutex<Option<LlmUsage>>>,
}

// ── Vector Store Stats ───────────────────────────────────────────────

/// Statistics returned by a vector store for admin display.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VectorStoreStats {
    pub backend: String,
    pub collection_name: String,
    pub vector_count: u64,
}

// ── Pipeline Progress ────────────────────────────────────────────────

/// Progress event emitted by the chat pipeline at each agent stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineProgress {
    pub stage: String,
    pub status: StageStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Model name used by this stage (e.g. "claude-sonnet-4-20250514").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Started,
    Done,
    Skipped,
    Error,
}

/// Sender half for pipeline progress events.
pub type ProgressSender = tokio::sync::mpsc::UnboundedSender<PipelineProgress>;

/// Side-channel for rich pipeline metadata, populated incrementally
/// by `ChatPipeline::process()` and consumed by the inference logger.
pub type MetadataCell = Arc<Mutex<PipelineMetadata>>;

/// Metadata collected during pipeline execution for inference logging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineMetadata {
    pub intent: Option<String>,
    pub language: Option<String>,
    pub complexity: Option<String>,
    pub pipeline_route: Option<String>,
    pub self_rag_decision: Option<String>,
    pub self_rag_confidence: Option<f32>,
    pub chunks_retrieved: Option<u32>,
    pub avg_chunk_score: Option<f32>,
    pub quality_guard_pass: Option<bool>,
    pub relevance_score: Option<f32>,
    pub hallucination_score: Option<f32>,
    pub completeness_score: Option<f32>,
    pub search_ms: Option<u64>,
    pub generation_ms: Option<u64>,
}

// ── OpenAI-Compatible Chat Types ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// An image attachment for vision-capable LLMs.
#[derive(Debug, Clone)]
pub struct ImageContent {
    /// Base64-encoded image data.
    pub base64_data: String,
    /// MIME type of the image (e.g. "image/png", "image/jpeg", "application/pdf").
    pub media_type: String,
}

/// A message that can contain both text and images for vision models.
#[derive(Debug, Clone)]
pub struct VisionMessage {
    pub role: String,
    pub text: String,
    pub images: Vec<ImageContent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: ChatUsage,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ── Streaming Chunk Types (SSE) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChunkChoice {
    pub index: usize,
    pub delta: ChatChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ── Document & Search Types ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub chunk_id: ChunkId,
    pub doc_id: DocId,
    pub workspace_id: WorkspaceId,
    pub content: String,
    pub chunk_index: usize,
    pub embedding: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ChunkMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChunkMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f32>,
    /// Page numbers this chunk spans (1-indexed). Present for page-aware formats like PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_numbers: Option<Vec<usize>>,
    // ── Enrichment fields (populated by Chunk Enricher agent) ────
    /// Context prefix (e.g., "From: Tax Policy 2025, Section 3.2")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_prefix: Option<String>,
    /// One-sentence summary of the chunk
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Extracted search keywords (Thai and English)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    /// Hypothetical queries this chunk answers (HyDE)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothetical_queries: Option<Vec<String>>,
    /// Original content before enrichment (for display in search results)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_content: Option<String>,
}

// ── AI Document Preprocessing Types ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentAnalysis {
    pub primary_language: String,
    pub content_type: ContentType,
    pub structure_level: StructureLevel,
    pub needs_ocr_correction: bool,
    pub has_headers_footers: bool,
    pub estimated_sections: usize,
    pub confidence: f32,
    /// AI-recommended quality threshold (0.0–1.0) based on document characteristics.
    #[serde(default)]
    pub recommended_quality_threshold: Option<f32>,
    /// AI-recommended max chunk size (chars) based on content structure.
    #[serde(default)]
    pub recommended_max_chunk_size: Option<usize>,
    /// AI-recommended min document size (bytes) to bother with AI processing.
    #[serde(default)]
    pub recommended_min_ai_size: Option<usize>,
}

impl Default for DocumentAnalysis {
    fn default() -> Self {
        Self {
            primary_language: "en".into(),
            content_type: ContentType::Narrative,
            structure_level: StructureLevel::Unstructured,
            needs_ocr_correction: false,
            has_headers_footers: false,
            estimated_sections: 1,
            confidence: 0.0,
            recommended_quality_threshold: None,
            recommended_max_chunk_size: None,
            recommended_min_ai_size: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Narrative,
    Tabular,
    Mixed,
    Form,
    Slides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructureLevel {
    WellStructured,
    SemiStructured,
    Unstructured,
}

#[derive(Debug, Clone)]
pub struct ConvertedDocument {
    pub markdown: String,
    pub analysis: DocumentAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub overall_score: f32,
    pub coherence_score: f32,
    pub completeness_score: f32,
    pub formatting_score: f32,
    #[serde(default)]
    pub issues: Vec<String>,
    pub passed: bool,
}

#[derive(Debug, Clone)]
pub struct EnrichedChunk {
    pub content: String,
    pub topic: Option<String>,
    pub section_title: Option<String>,
    pub language: Option<String>,
    pub chunk_type: Option<String>,
    /// Page numbers this chunk spans (1-indexed).
    pub page_numbers: Option<Vec<usize>>,
}

// ── Orchestrator Agent Types ────────────────────────────────────────

/// Which pipeline agent the orchestrator refers to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineAgent {
    Analyzer,
    Converter,
    QualityChecker,
    Chunker,
}

/// Parameter overrides the orchestrator can suggest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestratorParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_threshold: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chunk_size: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt_size: Option<usize>,
}

/// What the orchestrator decides after reviewing an agent's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OrchestratorAction {
    /// Accept the result and proceed to the next stage.
    Accept,
    /// Retry the same agent with adjustments.
    Retry {
        #[serde(default)]
        adjustments: Vec<String>,
        #[serde(default)]
        params: Option<OrchestratorParams>,
    },
    /// Skip this stage and proceed with current results.
    Skip { reason: String },
    /// Fall back to mechanical processing entirely.
    FallbackMechanical { reason: String },
    /// Accept but flag for human review.
    FlagForReview { reason: String },
    /// Adjust parameters for upcoming stages (and proceed).
    AdjustParams { params: OrchestratorParams },
}

/// A single orchestrator decision with reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorDecision {
    #[serde(flatten)]
    pub action: OrchestratorAction,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_confidence() -> f32 {
    0.5
}

/// Snapshot of pipeline state for the orchestrator's decision.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineSnapshot {
    pub completed_stage: String,
    pub analysis_confidence: Option<f32>,
    pub analysis_language: Option<String>,
    pub analysis_content_type: Option<String>,
    pub quality_overall: Option<f32>,
    pub quality_issues: Option<Vec<String>>,
    pub chunk_count: Option<usize>,
    pub chunk_issues: Option<Vec<String>>,
    pub orchestrator_call_count: u32,
    pub max_orchestrator_calls: u32,
    pub decision_history: Vec<String>,
    pub effective_quality_threshold: f32,
    pub effective_max_chunk_size: usize,
    pub doc_size_bytes: usize,
    pub mime_type: String,
    pub needs_ocr_correction: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub top_k: usize,
    pub workspace_ids: Vec<WorkspaceId>,
    /// When true and workspace_ids is empty, search returns all results (no filter).
    /// When false and workspace_ids is empty, search returns no results (no access).
    pub unrestricted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub chunk: DocumentChunk,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryIntent {
    Retrieval,
    DirectAnswer,
    Clarification,
}

// ── MCP Connector Types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    /// Local process via stdin/stdout.
    Stdio,
    /// Remote server via SSE/HTTP.
    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorStatus {
    Active,
    Paused,
    Error,
    Syncing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Manual trigger only.
    OnDemand,
    /// Periodic scheduled sync.
    Scheduled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Configuration for a connector to an external MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConnectorConfig {
    pub id: ConnectorId,
    pub name: String,
    pub description: String,
    pub transport: McpTransport,
    /// For stdio: the command to spawn (e.g., "npx @anthropic/mcp-server-confluence").
    #[serde(default)]
    pub command: Option<String>,
    /// For stdio: command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// For stdio: environment variables to pass to the child process.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// For SSE: the server URL.
    #[serde(default)]
    pub url: Option<String>,
    /// For SSE: optional auth headers.
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    /// Target workspace to ingest content into.
    pub workspace_id: WorkspaceId,
    pub sync_mode: SyncMode,
    /// Cron expression for scheduled sync (e.g., "0 */6 * * *").
    #[serde(default)]
    pub schedule_cron: Option<String>,
    /// Resource URI patterns to include (glob-like filters).
    #[serde(default)]
    pub resource_filters: Vec<String>,
    /// Maximum items to sync per run.
    #[serde(default)]
    pub max_items_per_sync: Option<usize>,
    /// Pre-configured tool calls for tool-based sources (Slack, Web, DB).
    #[serde(default)]
    pub tool_calls: Vec<ToolCallConfig>,
    /// Webhook URL to notify on sync completion/failure.
    #[serde(default)]
    pub webhook_url: Option<String>,
    /// Shared secret sent as Bearer token in webhook Authorization header.
    #[serde(default)]
    pub webhook_secret: Option<String>,
    pub status: ConnectorStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Pre-configured tool call executed during sync (for tool-based MCP sources).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallConfig {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    /// JSON path to extract content from the tool result.
    #[serde(default)]
    pub result_content_path: Option<String>,
    /// MIME type to assign to extracted content.
    #[serde(default = "default_mime_type")]
    pub result_mime_type: String,
    /// Title template (can use {index}, {date}).
    #[serde(default)]
    pub title_template: String,
}

fn default_mime_type() -> String {
    "text/plain".into()
}

/// Tracks sync state for a single MCP resource (change detection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub connector_id: ConnectorId,
    /// MCP resource URI (unique identifier from the MCP server).
    pub resource_uri: String,
    /// SHA-256 content hash for change detection.
    pub content_hash: String,
    /// The DocId in ThaiRAG's KM store for this resource.
    pub doc_id: Option<DocId>,
    pub last_synced_at: chrono::DateTime<chrono::Utc>,
    /// MCP-provided metadata (e.g., last modified timestamp from source).
    #[serde(default)]
    pub source_metadata: Option<serde_json::Value>,
}

/// A record of a single sync execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRun {
    pub id: SyncRunId,
    pub connector_id: ConnectorId,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub status: SyncRunStatus,
    pub items_discovered: usize,
    pub items_created: usize,
    pub items_updated: usize,
    pub items_skipped: usize,
    pub items_failed: usize,
    pub error_message: Option<String>,
}

/// A resource discovered from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub description: Option<String>,
}

/// Content read from an MCP resource.
#[derive(Debug, Clone)]
pub struct McpResourceContent {
    pub uri: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Tool info from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

// ── Context Compaction & Personal Memory ─────────────────────────────

/// Type of personal memory extracted from conversations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PersonalMemoryType {
    /// User preference (e.g., "prefers bullet points")
    Preference,
    /// Factual info about the user (e.g., "works in HR")
    Fact,
    /// Decision made during conversation (e.g., "chose PostgreSQL")
    Decision,
    /// General conversation summary
    Conversation,
    /// User correction (e.g., "deadline is Friday not Thursday")
    Correction,
}

/// A personal memory entry stored in the vector database per user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalMemory {
    pub id: MemoryId,
    pub user_id: UserId,
    pub memory_type: PersonalMemoryType,
    pub summary: String,
    pub topics: Vec<String>,
    pub importance: f32,
    pub created_at: i64,
    pub last_accessed_at: i64,
    /// Relevance score that decays over time (0.0–1.0).
    pub relevance_score: f32,
}

/// Result of context compaction — the compacted session state.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// Summary of compacted messages (injected as system message).
    pub summary: String,
    /// Personal memories extracted during compaction.
    pub extracted_memories: Vec<PersonalMemory>,
    /// Number of messages that were compacted.
    pub messages_compacted: usize,
    /// Number of messages kept intact (recent).
    pub messages_kept: usize,
}

// ── Job Queue Types ──────────────────────────────────────────────────

/// The kind of background job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Process a newly uploaded document (convert → chunk → embed → index).
    DocumentIngestion,
    /// Reprocess an existing document (re-chunk + re-embed).
    DocumentReprocess,
    /// Reprocess all documents in a workspace.
    BatchReprocess,
    /// Batch upload of multiple documents (CSV or ZIP).
    BatchUpload,
    /// Refresh a document from its source URL on schedule.
    DocumentRefresh,
}

impl std::fmt::Display for JobKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DocumentIngestion => write!(f, "document_ingestion"),
            Self::DocumentReprocess => write!(f, "document_reprocess"),
            Self::BatchReprocess => write!(f, "batch_reprocess"),
            Self::BatchUpload => write!(f, "batch_upload"),
            Self::DocumentRefresh => write!(f, "document_refresh"),
        }
    }
}

/// Status of a background job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "queued"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A background job tracked by the job queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    pub status: JobStatus,
    pub workspace_id: WorkspaceId,
    /// Related document ID (if applicable).
    pub doc_id: Option<DocId>,
    /// Human-readable description.
    pub description: String,
    /// Unix timestamp (seconds) when the job was created.
    pub created_at: i64,
    /// Unix timestamp when the job started running.
    pub started_at: Option<i64>,
    /// Unix timestamp when the job completed/failed.
    pub completed_at: Option<i64>,
    /// Error message if the job failed.
    pub error: Option<String>,
    /// Number of items processed (e.g., chunks indexed).
    pub items_processed: usize,
    /// Total number of items to process (for progress tracking in batch jobs).
    #[serde(default)]
    pub items_total: Option<usize>,
}

/// Estimate tokens for a string using heuristic: Thai ~2 chars/token, EN ~4 chars/token.
pub fn estimate_tokens(text: &str) -> usize {
    let mut thai_chars = 0usize;
    let mut other_chars = 0usize;
    for c in text.chars() {
        if ('\u{0E01}'..='\u{0E5B}').contains(&c) {
            thai_chars += 1;
        } else {
            other_chars += 1;
        }
    }
    (thai_chars / 2) + (other_chars / 4) + 1
}

// ── Webhook Notification Types ──────────────────────────────────────

/// Events that can trigger webhook notifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEvent {
    JobCompleted,
    JobFailed,
    DocumentIngested,
    SyncCompleted,
    SyncFailed,
}

impl std::fmt::Display for WebhookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JobCompleted => write!(f, "job_completed"),
            Self::JobFailed => write!(f, "job_failed"),
            Self::DocumentIngested => write!(f, "document_ingested"),
            Self::SyncCompleted => write!(f, "sync_completed"),
            Self::SyncFailed => write!(f, "sync_failed"),
        }
    }
}

/// A registered webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: WebhookId,
    pub url: String,
    /// HMAC-SHA256 secret for signing payloads.
    #[serde(default, skip_serializing)]
    pub secret: String,
    /// Which events this webhook subscribes to.
    pub events: Vec<WebhookEvent>,
    pub is_active: bool,
    pub created_at: String,
}

/// Payload sent to webhook endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub event: WebhookEvent,
    pub timestamp: String,
    pub data: serde_json::Value,
}

// ── Search Quality Evaluation Types ─────────────────────────────────

define_id!(EvalSetId);

/// A set of evaluation queries with ground-truth relevance judgments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalQuerySet {
    pub id: EvalSetId,
    pub name: String,
    pub queries: Vec<EvalQuery>,
    pub created_at: String,
}

/// A single evaluation query with known relevant documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalQuery {
    pub query: String,
    pub relevant_doc_ids: Vec<DocId>,
    /// Graded relevance scores (same order as relevant_doc_ids).
    /// If None, binary relevance (1.0 for all relevant docs) is assumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance_scores: Option<Vec<f32>>,
}

/// Result of running an evaluation query set against the search pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub query_set_id: EvalSetId,
    pub run_at: String,
    pub metrics: EvalMetrics,
    pub per_query: Vec<QueryEvalResult>,
}

/// Aggregate metrics across all queries in an evaluation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalMetrics {
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
    pub mrr: f64,
    pub precision_at_5: f64,
    pub precision_at_10: f64,
    pub recall_at_10: f64,
    pub mean_latency_ms: f64,
}

/// Per-query evaluation metrics from a single evaluation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEvalResult {
    pub query: String,
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
    pub mrr: f64,
    pub precision: f64,
    pub recall: f64,
    pub latency_ms: u64,
    pub retrieved_doc_ids: Vec<DocId>,
}
