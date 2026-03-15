use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

// ── ID Newtypes ──────────────────────────────────────────────────────

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    Skip {
        reason: String,
    },
    /// Fall back to mechanical processing entirely.
    FallbackMechanical {
        reason: String,
    },
    /// Accept but flag for human review.
    FlagForReview {
        reason: String,
    },
    /// Adjust parameters for upcoming stages (and proceed).
    AdjustParams {
        params: OrchestratorParams,
    },
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
