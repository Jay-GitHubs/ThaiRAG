use async_trait::async_trait;

use crate::error::Result;
use crate::types::{ChatMessage, DocumentChunk, SearchQuery, SearchResult};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String>;
    fn model_name(&self) -> &str;
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
