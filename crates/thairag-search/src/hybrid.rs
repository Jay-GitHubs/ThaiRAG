use std::collections::HashMap;
use std::sync::Arc;

use thairag_config::schema::SearchConfig;
use thairag_core::error::Result;
use thairag_core::traits::{EmbeddingModel, Reranker, TextSearch, VectorStore};
use thairag_core::types::{DocumentChunk, SearchQuery, SearchResult};

/// Hybrid search engine combining vector similarity and BM25 text search.
/// Uses Reciprocal Rank Fusion (RRF) for merging, then reranking.
pub struct HybridSearchEngine {
    embedding: Arc<dyn EmbeddingModel>,
    vector_store: Arc<dyn VectorStore>,
    text_search: Arc<dyn TextSearch>,
    reranker: Arc<dyn Reranker>,
    config: SearchConfig,
}

impl HybridSearchEngine {
    pub fn new(
        embedding: Arc<dyn EmbeddingModel>,
        vector_store: Arc<dyn VectorStore>,
        text_search: Arc<dyn TextSearch>,
        reranker: Arc<dyn Reranker>,
        config: SearchConfig,
    ) -> Self {
        Self {
            embedding,
            vector_store,
            text_search,
            reranker,
            config,
        }
    }

    /// Index document chunks into both vector store and text search.
    pub async fn index_chunks(&self, chunks: &[DocumentChunk]) -> Result<()> {
        // Embed chunks
        let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let embeddings = self.embedding.embed(&texts).await?;

        let mut embedded_chunks: Vec<DocumentChunk> = chunks.to_vec();
        for (chunk, emb) in embedded_chunks.iter_mut().zip(embeddings) {
            chunk.embedding = Some(emb);
        }

        // Store in both backends
        let vector_fut = self.vector_store.upsert(&embedded_chunks);
        let text_fut = self.text_search.index(&embedded_chunks);

        let (v_res, t_res) = tokio::join!(vector_fut, text_fut);
        v_res?;
        t_res?;

        Ok(())
    }

    /// Hybrid search: parallel vector + BM25, RRF merge, rerank.
    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // Embed the query
        let query_embeddings = self.embedding.embed(std::slice::from_ref(&query.text)).await?;
        let query_embedding = &query_embeddings[0];

        // Parallel search
        let vector_query = SearchQuery {
            text: query.text.clone(),
            top_k: self.config.top_k,
            workspace_ids: query.workspace_ids.clone(),
        };
        let text_query = vector_query.clone();

        let vector_fut = self.vector_store.search(query_embedding, &vector_query);
        let text_fut = self.text_search.search(&text_query);

        let (vector_results, text_results) = tokio::join!(vector_fut, text_fut);
        let vector_results = vector_results?;
        let text_results = text_results?;

        // RRF merge
        let merged = self.rrf_merge(&vector_results, &text_results);

        // Rerank
        let top = merged.into_iter().take(self.config.rerank_top_k).collect();
        self.reranker.rerank(&query.text, top).await
    }

    fn rrf_merge(
        &self,
        vector_results: &[SearchResult],
        text_results: &[SearchResult],
    ) -> Vec<SearchResult> {
        let k = self.config.rrf_k as f32;
        let mut scores: HashMap<String, (f32, SearchResult)> = HashMap::new();

        for (rank, result) in vector_results.iter().enumerate() {
            let rrf_score = self.config.vector_weight / (k + rank as f32 + 1.0);
            let key = result.chunk.chunk_id.to_string();
            scores
                .entry(key)
                .and_modify(|(s, _)| *s += rrf_score)
                .or_insert((rrf_score, result.clone()));
        }

        for (rank, result) in text_results.iter().enumerate() {
            let rrf_score = self.config.text_weight / (k + rank as f32 + 1.0);
            let key = result.chunk.chunk_id.to_string();
            scores
                .entry(key)
                .and_modify(|(s, _)| *s += rrf_score)
                .or_insert((rrf_score, result.clone()));
        }

        let mut merged: Vec<SearchResult> = scores
            .into_values()
            .map(|(score, mut result)| {
                result.score = score;
                result
            })
            .collect();

        merged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        merged
    }
}
