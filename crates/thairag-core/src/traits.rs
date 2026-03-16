use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{
    ChatMessage, ConvertedDocument, DocumentAnalysis, DocumentChunk, EnrichedChunk, LlmResponse,
    LlmStreamResponse, QualityReport, SearchQuery, SearchResult, VisionMessage,
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
}

#[async_trait]
pub trait TextSearch: Send + Sync {
    async fn index(&self, chunks: &[DocumentChunk]) -> Result<()>;
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;
    async fn delete_by_doc(&self, doc_id: crate::types::DocId) -> Result<()>;
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
