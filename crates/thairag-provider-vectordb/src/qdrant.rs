use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{DocId, DocumentChunk, SearchQuery, SearchResult};

pub struct QdrantVectorStore {
    _url: String,
    _collection: String,
}

impl QdrantVectorStore {
    pub fn new(url: &str, collection: &str) -> Self {
        Self {
            _url: url.to_string(),
            _collection: collection.to_string(),
        }
    }
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    async fn upsert(&self, _chunks: &[DocumentChunk]) -> Result<()> {
        todo!("Qdrant upsert not yet implemented")
    }

    async fn search(&self, _embedding: &[f32], _query: &SearchQuery) -> Result<Vec<SearchResult>> {
        todo!("Qdrant search not yet implemented")
    }

    async fn delete_by_doc(&self, _doc_id: DocId) -> Result<()> {
        todo!("Qdrant delete_by_doc not yet implemented")
    }
}
