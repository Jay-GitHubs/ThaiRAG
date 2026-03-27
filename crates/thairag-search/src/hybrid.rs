use std::collections::HashMap;
use std::sync::Arc;

use thairag_config::schema::SearchConfig;
use thairag_core::error::Result;
use thairag_core::traits::{EmbeddingModel, Reranker, SearchPlugin, TextSearch, VectorStore};
use thairag_core::types::{DocId, DocumentChunk, SearchQuery, SearchResult};

/// Hybrid search engine combining vector similarity and BM25 text search.
/// Uses Reciprocal Rank Fusion (RRF) for merging, then reranking.
pub struct HybridSearchEngine {
    embedding: Arc<dyn EmbeddingModel>,
    vector_store: Arc<dyn VectorStore>,
    text_search: Arc<dyn TextSearch>,
    reranker: Arc<dyn Reranker>,
    config: SearchConfig,
    /// Optional search plugins applied pre/post search.
    search_plugins: Vec<Arc<dyn SearchPlugin>>,
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
            search_plugins: Vec::new(),
        }
    }

    /// Create a new engine with search plugins for pre/post-processing.
    pub fn new_with_plugins(
        embedding: Arc<dyn EmbeddingModel>,
        vector_store: Arc<dyn VectorStore>,
        text_search: Arc<dyn TextSearch>,
        reranker: Arc<dyn Reranker>,
        config: SearchConfig,
        search_plugins: Vec<Arc<dyn SearchPlugin>>,
    ) -> Self {
        Self {
            embedding,
            vector_store,
            text_search,
            reranker,
            config,
            search_plugins,
        }
    }

    /// Set search plugins to be applied during search.
    pub fn set_search_plugins(&mut self, plugins: Vec<Arc<dyn SearchPlugin>>) {
        self.search_plugins = plugins;
    }

    /// Number of documents in the text search index.
    pub fn text_search_doc_count(&self) -> u64 {
        self.text_search.doc_count()
    }

    /// Index document chunks into both vector store and text search.
    ///
    /// When chunks have enrichment metadata (context_prefix, keywords,
    /// hypothetical_queries), the embedding text is augmented with this
    /// metadata for better retrieval, while the stored content is preserved.
    pub async fn index_chunks(&self, chunks: &[DocumentChunk]) -> Result<()> {
        // Build enriched texts for embedding — includes metadata for better recall
        let texts: Vec<String> = chunks.iter().map(Self::enriched_text).collect();
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

    /// Re-index chunks into text search only (skip vector store).
    /// Used at startup to rebuild Tantivy from stored chunks.
    pub async fn reindex_text_search(&self, chunks: &[DocumentChunk]) -> Result<()> {
        self.text_search.index(chunks).await
    }

    /// Build enriched text for embedding by prepending context and appending
    /// keywords + hypothetical queries from chunk metadata.
    fn enriched_text(chunk: &DocumentChunk) -> String {
        let meta = match &chunk.metadata {
            Some(m) => m,
            None => return chunk.content.clone(),
        };

        let has_enrichment = meta.context_prefix.is_some()
            || meta.keywords.as_ref().is_some_and(|k| !k.is_empty())
            || meta
                .hypothetical_queries
                .as_ref()
                .is_some_and(|h| !h.is_empty());

        if !has_enrichment {
            return chunk.content.clone();
        }

        let mut text = String::new();

        // Prepend context (e.g., "From: Tax Policy 2025, Section 3.2")
        if let Some(ref ctx) = meta.context_prefix {
            text.push_str(ctx);
            text.push('\n');
        }

        // Main content
        text.push_str(&chunk.content);

        // Append keywords for broader term matching
        if let Some(ref kw) = meta.keywords
            && !kw.is_empty()
        {
            text.push_str("\nKeywords: ");
            text.push_str(&kw.join(", "));
        }

        // Append hypothetical queries (HyDE) for query-aware embedding
        if let Some(ref hq) = meta.hypothetical_queries
            && !hq.is_empty()
        {
            text.push_str("\nQueries: ");
            text.push_str(&hq.join(" | "));
        }

        text
    }

    /// Hybrid search: parallel vector + BM25, RRF merge, rerank.
    ///
    /// If search plugins are registered, `pre_search` is applied to the query
    /// text before searching, and `post_search` is applied to the final results.
    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // Apply search plugin pre-processing (query expansion, etc.)
        let effective_query_text = if self.search_plugins.is_empty() {
            query.text.clone()
        } else {
            let mut q = query.text.clone();
            for plugin in &self.search_plugins {
                q = plugin.pre_search(&q);
            }
            q
        };

        // Embed the query
        let query_embeddings = self
            .embedding
            .embed(std::slice::from_ref(&effective_query_text))
            .await?;
        let query_embedding = &query_embeddings[0];

        // Parallel search
        let vector_query = SearchQuery {
            text: effective_query_text.clone(),
            top_k: self.config.top_k,
            workspace_ids: query.workspace_ids.clone(),
            unrestricted: query.unrestricted,
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
        let mut results = self.reranker.rerank(&query.text, top).await?;

        // Apply search plugin post-processing (filtering, re-ranking, etc.)
        if !self.search_plugins.is_empty() {
            for plugin in &self.search_plugins {
                results = plugin.post_search(results);
            }
        }

        Ok(results)
    }

    /// Delete a document from both vector store and text search.
    pub async fn delete_doc(&self, doc_id: DocId) -> Result<()> {
        let (v_res, t_res) = tokio::join!(
            self.vector_store.delete_by_doc(doc_id),
            self.text_search.delete_by_doc(doc_id),
        );
        v_res?;
        t_res?;
        Ok(())
    }

    /// Delete all vectors from the vector store (used when embedding model changes).
    pub async fn delete_all_vectors(&self) -> Result<()> {
        self.vector_store.delete_all().await
    }

    /// Return statistics about the underlying vector store.
    pub async fn vector_store_stats(&self) -> Result<thairag_core::types::VectorStoreStats> {
        self.vector_store.collection_stats().await
    }

    pub(crate) fn rrf_merge(
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

        merged.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Normalize RRF scores to 0–1 range so downstream confidence
        // thresholds (calibrated for cosine similarity) work correctly.
        // Without normalization, RRF scores are ~0.001–0.05 (because the
        // formula is weight/(k+rank+1) with k=60) and would always trigger
        // false "low confidence" anti-hallucination guards.
        if let Some(max) = merged.first().map(|r| r.score)
            && max > 0.0
        {
            for r in &mut merged {
                r.score /= max;
            }
        }

        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use thairag_core::types::{ChunkId, DocId, WorkspaceId};
    use uuid::Uuid;

    // ── Mocks ────────────────────────────────────────────────────────

    struct MockEmbedding;
    #[async_trait]
    impl EmbeddingModel for MockEmbedding {
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(vec![vec![0.0; 3]])
        }
        fn dimension(&self) -> usize {
            3
        }
    }

    struct MockVectorStore;
    #[async_trait]
    impl VectorStore for MockVectorStore {
        async fn upsert(&self, _chunks: &[DocumentChunk]) -> Result<()> {
            Ok(())
        }
        async fn search(
            &self,
            _embedding: &[f32],
            _query: &SearchQuery,
        ) -> Result<Vec<SearchResult>> {
            Ok(vec![])
        }
        async fn delete_by_doc(&self, _doc_id: thairag_core::types::DocId) -> Result<()> {
            Ok(())
        }
    }

    struct MockTextSearch;
    #[async_trait]
    impl TextSearch for MockTextSearch {
        async fn index(&self, _chunks: &[DocumentChunk]) -> Result<()> {
            Ok(())
        }
        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            Ok(vec![])
        }
        async fn delete_by_doc(&self, _doc_id: thairag_core::types::DocId) -> Result<()> {
            Ok(())
        }
    }

    struct MockReranker;
    #[async_trait]
    impl Reranker for MockReranker {
        async fn rerank(
            &self,
            _query: &str,
            results: Vec<SearchResult>,
        ) -> Result<Vec<SearchResult>> {
            Ok(results)
        }
    }

    fn make_result(id: &str, score: f32) -> SearchResult {
        SearchResult {
            chunk: DocumentChunk {
                chunk_id: ChunkId(Uuid::parse_str(id).unwrap()),
                doc_id: DocId::new(),
                workspace_id: WorkspaceId::new(),
                content: format!("chunk-{id}"),
                chunk_index: 0,
                embedding: None,
                metadata: None,
            },
            score,
        }
    }

    fn build_engine(rrf_k: usize, vw: f32, tw: f32) -> HybridSearchEngine {
        HybridSearchEngine::new(
            Arc::new(MockEmbedding),
            Arc::new(MockVectorStore),
            Arc::new(MockTextSearch),
            Arc::new(MockReranker),
            SearchConfig {
                top_k: 10,
                rerank_top_k: 5,
                rrf_k,
                vector_weight: vw,
                text_weight: tw,
            },
        )
    }

    // ── RRF Merge Tests ──────────────────────────────────────────────

    #[test]
    fn rrf_both_empty() {
        let engine = build_engine(60, 0.5, 0.5);
        let merged = engine.rrf_merge(&[], &[]);
        assert!(merged.is_empty());
    }

    #[test]
    fn rrf_vector_only() {
        let engine = build_engine(60, 1.0, 0.0);
        let id = "00000000-0000-0000-0000-000000000001";
        let vec_results = vec![make_result(id, 0.9)];
        let merged = engine.rrf_merge(&vec_results, &[]);
        assert_eq!(merged.len(), 1);
        // Single result is normalized to 1.0
        assert!((merged[0].score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rrf_text_only() {
        let engine = build_engine(60, 0.0, 1.0);
        let id = "00000000-0000-0000-0000-000000000002";
        let text_results = vec![make_result(id, 0.8)];
        let merged = engine.rrf_merge(&[], &text_results);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rrf_shared_doc_gets_higher_score() {
        let engine = build_engine(60, 0.5, 0.5);
        let shared_id = "00000000-0000-0000-0000-000000000003";
        let unique_id = "00000000-0000-0000-0000-000000000004";

        let vec_results = vec![make_result(shared_id, 0.9), make_result(unique_id, 0.7)];
        let text_results = vec![make_result(shared_id, 0.8)];

        let merged = engine.rrf_merge(&vec_results, &text_results);
        assert_eq!(merged.len(), 2);
        // The shared doc should be ranked first (higher combined RRF score)
        assert_eq!(merged[0].chunk.chunk_id.0.to_string(), shared_id);
    }

    #[test]
    fn rrf_descending_order() {
        let engine = build_engine(60, 0.5, 0.5);
        let id_a = "00000000-0000-0000-0000-00000000000a";
        let id_b = "00000000-0000-0000-0000-00000000000b";

        // a at rank 0 in both, b at rank 1 in both → a should score higher
        let vec_results = vec![make_result(id_a, 0.9), make_result(id_b, 0.7)];
        let text_results = vec![make_result(id_a, 0.9), make_result(id_b, 0.7)];

        let merged = engine.rrf_merge(&vec_results, &text_results);
        assert!(merged[0].score >= merged[1].score);
    }

    #[test]
    fn rrf_score_uses_weights() {
        // With normalization, a single result always scores 1.0.
        // To test weight influence, use two results — the *ratio* between
        // scores changes when a result appears in both lists (weighted).
        let engine = build_engine(60, 0.7, 0.3);
        let id_a = "00000000-0000-0000-0000-000000000005";
        let id_b = "00000000-0000-0000-0000-000000000006";

        // a in both lists, b only in text → vector-heavy weight should favor a
        let vec_results = vec![make_result(id_a, 0.9)];
        let text_results = vec![make_result(id_a, 0.8), make_result(id_b, 0.7)];

        let merged = engine.rrf_merge(&vec_results, &text_results);
        assert_eq!(merged.len(), 2);
        // a (in both) should have a higher normalized score than b (text only)
        assert_eq!(merged[0].chunk.chunk_id.0.to_string(), id_a);
        assert!(merged[0].score > merged[1].score);
    }

    #[test]
    fn rrf_scores_normalized_to_0_1() {
        let engine = build_engine(60, 0.5, 0.5);
        let ids: Vec<String> = (1..=5)
            .map(|i| format!("00000000-0000-0000-0000-{i:012}"))
            .collect();

        let vec_results: Vec<SearchResult> = ids.iter().map(|id| make_result(id, 0.9)).collect();
        let text_results: Vec<SearchResult> =
            ids[0..3].iter().map(|id| make_result(id, 0.8)).collect();

        let merged = engine.rrf_merge(&vec_results, &text_results);

        // Top result should be 1.0, all others in (0, 1]
        assert!((merged[0].score - 1.0).abs() < f32::EPSILON);
        for r in &merged {
            assert!(
                r.score > 0.0 && r.score <= 1.0,
                "score {} out of 0–1",
                r.score
            );
        }
    }
}
