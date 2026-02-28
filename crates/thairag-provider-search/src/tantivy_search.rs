use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::TextSearch;
use thairag_core::types::{DocumentChunk, SearchQuery, SearchResult};

pub struct TantivySearch {
    _index_path: String,
}

impl TantivySearch {
    pub fn new(index_path: &str) -> Self {
        Self {
            _index_path: index_path.to_string(),
        }
    }
}

#[async_trait]
impl TextSearch for TantivySearch {
    async fn index(&self, _chunks: &[DocumentChunk]) -> Result<()> {
        todo!("Tantivy indexing not yet implemented")
    }

    async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
        todo!("Tantivy search not yet implemented")
    }
}
