use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{
    ChatMessage, ConvertedDocument, DocumentAnalysis, DocumentChunk, EnrichedChunk, Job, JobId,
    JobStatus, LlmResponse, LlmStreamResponse, McpResource, McpResourceContent, McpToolInfo,
    MemoryId, PersonalMemory, QualityReport, SearchQuery, SearchResult, SessionId, UserId,
    VisionMessage, WorkspaceId,
};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse>;

    async fn generate_stream(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmStreamResponse> {
        let resp = self.generate(messages, max_tokens).await?;
        let usage = Arc::new(Mutex::new(Some(resp.usage)));
        Ok(LlmStreamResponse {
            stream: Box::pin(tokio_stream::once(Ok(resp.content))),
            usage,
        })
    }

    fn model_name(&self) -> &str;

    /// Whether this provider's current model supports vision (image) input.
    fn supports_vision(&self) -> bool {
        false
    }

    /// Generate a response from messages containing images.
    /// Default implementation ignores images and falls back to text-only.
    async fn generate_vision(
        &self,
        messages: &[VisionMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        // Fallback: convert to text-only messages
        let text_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.text.clone(),
            })
            .collect();
        self.generate(&text_messages, max_tokens).await
    }
}

#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()>;
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>>;
    async fn delete_by_doc(&self, doc_id: crate::types::DocId) -> Result<()>;
    /// Delete all vectors (used when embedding model changes and re-indexing is needed).
    async fn delete_all(&self) -> Result<()> {
        Ok(()) // default no-op for backwards compatibility
    }
    /// Return statistics about the vector store (backend type, collection name, vector count).
    async fn collection_stats(&self) -> Result<crate::types::VectorStoreStats> {
        Ok(crate::types::VectorStoreStats::default())
    }
}

#[async_trait]
pub trait TextSearch: Send + Sync {
    async fn index(&self, chunks: &[DocumentChunk]) -> Result<()>;
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;
    async fn delete_by_doc(&self, doc_id: crate::types::DocId) -> Result<()>;
    /// Number of documents in the index. Used by startup rebuild logic.
    fn doc_count(&self) -> u64 {
        0
    }
}

#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(&self, query: &str, results: Vec<SearchResult>) -> Result<Vec<SearchResult>>;
}

pub trait DocumentProcessor: Send + Sync {
    fn convert(&self, raw: &[u8], mime_type: &str) -> Result<String>;
}

pub trait ThaiTokenizer: Send + Sync {
    fn tokenize(&self, text: &str) -> Vec<String>;
}

pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str, max_size: usize, overlap: usize) -> Vec<String>;
}

// ── Personal Memory Store ────────────────────────────────────────────

#[async_trait]
pub trait PersonalMemoryStore: Send + Sync {
    /// Store a personal memory (embeds and upserts to vector store).
    async fn store(&self, memory: &PersonalMemory, embedding: Vec<f32>) -> Result<()>;

    /// Search relevant memories for a user given a query embedding.
    async fn search(
        &self,
        user_id: UserId,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<PersonalMemory>>;

    /// Delete a specific memory by ID.
    async fn delete(&self, id: MemoryId) -> Result<()>;

    /// Delete all memories for a user.
    async fn delete_all_for_user(&self, user_id: UserId) -> Result<()>;

    /// Apply relevance decay to all memories older than the given age (in seconds).
    async fn apply_decay(&self, decay_factor: f32, min_score: f32) -> Result<usize>;
}

// ── MCP Client Trait ─────────────────────────────────────────────────

#[async_trait]
pub trait McpClient: Send + Sync {
    /// Connect to the MCP server and perform initialization handshake.
    async fn connect(&mut self) -> Result<()>;

    /// List available resources from the MCP server.
    async fn list_resources(&self) -> Result<Vec<McpResource>>;

    /// Read a specific resource by URI, returning raw content + mime type.
    async fn read_resource(&self, uri: &str) -> Result<McpResourceContent>;

    /// List available tools on the MCP server.
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>>;

    /// Call a tool on the MCP server.
    async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<serde_json::Value>;

    /// Disconnect / close the session.
    async fn disconnect(&mut self) -> Result<()>;
}

// ── AI Document Preprocessing Traits ─────────────────────────────────

#[async_trait]
pub trait DocumentAnalyzer: Send + Sync {
    async fn analyze(
        &self,
        raw_text: &str,
        mime_type: &str,
        doc_size_bytes: usize,
    ) -> Result<DocumentAnalysis>;
}

#[async_trait]
pub trait AiDocumentConverter: Send + Sync {
    async fn convert(
        &self,
        raw_text: &str,
        analysis: &DocumentAnalysis,
    ) -> Result<ConvertedDocument>;
}

#[async_trait]
pub trait QualityChecker: Send + Sync {
    async fn check(
        &self,
        original_text: &str,
        converted: &ConvertedDocument,
    ) -> Result<QualityReport>;
}

#[async_trait]
pub trait SmartChunker: Send + Sync {
    async fn chunk(
        &self,
        converted: &ConvertedDocument,
        max_chunk_size: usize,
    ) -> Result<Vec<EnrichedChunk>>;
}

// ── Session Store Trait ─────────────────────────────────────────────

#[async_trait]
pub trait SessionStoreTrait: Send + Sync {
    /// Get conversation history for a session.
    async fn get_history(&self, session_id: &SessionId) -> Option<Vec<ChatMessage>>;

    /// Append a user+assistant message pair to a session.
    async fn append(
        &self,
        session_id: SessionId,
        user_msg: ChatMessage,
        assistant_msg: ChatMessage,
        user_id: Option<UserId>,
    );

    /// Replace the session's message history (used by context compaction).
    async fn replace_messages(&self, session_id: &SessionId, new_messages: Vec<ChatMessage>);

    /// Get current message count for a session.
    async fn message_count(&self, session_id: &SessionId) -> usize;

    /// Number of active sessions.
    async fn count(&self) -> usize;

    /// Remove all sessions belonging to a specific user.
    async fn clear_user_sessions(&self, user_id: UserId) -> usize;

    /// Remove sessions idle longer than `max_age`.
    async fn cleanup_stale(&self, max_age: std::time::Duration);
}

// ── Embedding Cache Trait ───────────────────────────────────────────

#[async_trait]
pub trait EmbeddingCache: Send + Sync {
    /// Get a cached embedding for a text.
    async fn get(&self, text: &str) -> Option<Vec<f32>>;

    /// Get cached embeddings for multiple texts. Returns None for cache misses.
    async fn get_many(&self, texts: &[String]) -> Vec<Option<Vec<f32>>>;

    /// Store an embedding for a text.
    async fn put(&self, text: &str, embedding: Vec<f32>);

    /// Store multiple embeddings.
    async fn put_many(&self, pairs: Vec<(String, Vec<f32>)>);

    /// Number of cached entries.
    async fn len(&self) -> usize;

    /// Whether the cache is empty.
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

// ── Job Queue ────────────────────────────────────────────────────────

/// Trait for managing background job tracking.
#[async_trait]
pub trait JobQueue: Send + Sync {
    /// Submit a new job and return its ID.
    async fn enqueue(&self, job: Job) -> JobId;

    /// Get a job by ID.
    async fn get(&self, job_id: &JobId) -> Option<Job>;

    /// List jobs for a workspace, most recent first.
    async fn list_by_workspace(&self, workspace_id: &WorkspaceId) -> Vec<Job>;

    /// Update a job's status and optional error message.
    async fn update_status(&self, job_id: &JobId, status: JobStatus, error: Option<String>);

    /// Mark a job as running (sets started_at).
    async fn mark_running(&self, job_id: &JobId);

    /// Mark a job as completed (sets completed_at + items_processed).
    async fn mark_completed(&self, job_id: &JobId, items_processed: usize);

    /// Mark a job as failed (sets completed_at + error).
    async fn mark_failed(&self, job_id: &JobId, error: String);

    /// Increment the items_processed counter by 1.
    async fn increment_progress(&self, job_id: &JobId);

    /// Cancel a queued or running job.
    async fn cancel(&self, job_id: &JobId) -> bool;

    /// Remove completed/failed/cancelled jobs older than max_age.
    async fn cleanup(&self, max_age: std::time::Duration);

    /// Total number of tracked jobs.
    async fn count(&self) -> usize;
}
