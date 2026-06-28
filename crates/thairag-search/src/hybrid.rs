use std::collections::HashMap;
use std::sync::Arc;

use thairag_config::schema::SearchConfig;
use thairag_core::error::Result;
use thairag_core::traits::{
    EmbeddingModel, ImageEmbeddingModel, Reranker, SearchPlugin, TextSearch, VectorStore,
};
use thairag_core::types::{DocId, DocumentChunk, SearchQuery, SearchResult};

/// Optional CLIP visual-search side-channel: a multimodal embedding model paired
/// with a dedicated vector collection. When present, image chunks are embedded
/// into the shared CLIP space at index time and a text query is also matched
/// against image vectors, fusing visual hits into the result set.
struct ImageSearch {
    embedding: Arc<dyn ImageEmbeddingModel>,
    store: Arc<dyn VectorStore>,
    /// RRF fusion weight for image-vector hits relative to text/vector hits.
    weight: f32,
}

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
    /// Optional CLIP image-vector search (None = text-caption-only behaviour).
    image_search: Option<ImageSearch>,
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
            image_search: None,
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
            image_search: None,
        }
    }

    /// Set search plugins to be applied during search.
    pub fn set_search_plugins(&mut self, plugins: Vec<Arc<dyn SearchPlugin>>) {
        self.search_plugins = plugins;
    }

    /// Enable CLIP visual search by attaching an image-embedding model and a
    /// dedicated image-vector store. Builder-style so existing constructor call
    /// sites are untouched; left unset, the engine is text-caption-only.
    pub fn with_image_search(
        mut self,
        embedding: Arc<dyn ImageEmbeddingModel>,
        store: Arc<dyn VectorStore>,
        weight: f32,
    ) -> Self {
        self.image_search = Some(ImageSearch {
            embedding,
            store,
            weight,
        });
        self
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

        // Upsert precomputed CLIP image vectors into the image collection. Each
        // image chunk carries its vector in `metadata.image_embedding` (set at
        // ingest). We reuse the VectorStore contract by cloning the chunk with
        // its embedding swapped to the image vector, so the image collection is
        // keyed by the same chunk_id and retrieval maps straight back to chunks.
        if let Some(img) = &self.image_search {
            let image_chunks: Vec<DocumentChunk> = embedded_chunks
                .iter()
                .filter_map(|c| {
                    let vec = c.metadata.as_ref()?.image_embedding.clone()?;
                    let mut clone = c.clone();
                    clone.embedding = Some(vec);
                    Some(clone)
                })
                .collect();
            if !image_chunks.is_empty() {
                img.store.upsert(&image_chunks).await?;
            }
        }

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

        // The base text to embed: an explicit `embed_text` override wins over
        // raw `content`. This lets a table chunk store faithful HTML in
        // `content` (what the LLM reads) while embedding a clean, retrievable
        // row-linearized form here — avoiding indexing HTML tags as terms.
        let base = meta.embed_text.as_deref().unwrap_or(&chunk.content);

        let has_enrichment = meta.context_prefix.is_some()
            || meta.keywords.as_ref().is_some_and(|k| !k.is_empty())
            || meta
                .hypothetical_queries
                .as_ref()
                .is_some_and(|h| !h.is_empty());

        if !has_enrichment {
            return base.to_string();
        }

        let mut text = String::new();

        // Prepend context (e.g., "From: Tax Policy 2025, Section 3.2")
        if let Some(ref ctx) = meta.context_prefix {
            text.push_str(ctx);
            text.push('\n');
        }

        // Main content (embed override when present)
        text.push_str(base);

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
            query_images: Vec::new(),
            doc_ids: Vec::new(),
        };
        let text_query = vector_query.clone();

        let vector_fut = self.vector_store.search(query_embedding, &vector_query);
        let text_fut = self.text_search.search(&text_query);

        let (vector_results, text_results) = tokio::join!(vector_fut, text_fut);
        let vector_results = vector_results?;
        let text_results = text_results?;

        // CLIP visual search produces one or more rankings against the image
        // collection, all in the shared CLIP space. RRF merges by rank, so each
        // ranking fuses cleanly with the text/vector results.
        //   • text→image: embed the query text with the CLIP text encoder.
        //   • image→image: embed each attached image with the CLIP vision
        //     encoder (only when the request carried image bytes).
        let mut image_result_sets: Vec<Vec<SearchResult>> = Vec::new();
        if let Some(img) = &self.image_search {
            let clip_text = img
                .embedding
                .embed_query_text(std::slice::from_ref(&effective_query_text))
                .await?;
            if let Some(vec) = clip_text.first() {
                image_result_sets.push(img.store.search(vec, &vector_query).await?);
            }

            if !query.query_images.is_empty() {
                let clip_imgs = img.embedding.embed_images(&query.query_images).await?;
                for vec in &clip_imgs {
                    image_result_sets.push(img.store.search(vec, &vector_query).await?);
                }
            }
        }

        // RRF merge (image_result_sets empty when visual search is disabled)
        let merged = self.rrf_merge(&vector_results, &text_results, &image_result_sets);

        // Rerank. The reranker is a quality enhancer, not a correctness
        // dependency — if it fails (e.g. a flaky upstream rerank endpoint
        // returning 502), degrade to the un-reranked merged results rather than
        // failing the whole search/chat request.
        let top: Vec<SearchResult> = merged.into_iter().take(self.config.rerank_top_k).collect();
        let mut results = match self.reranker.rerank(&query.text, top.clone()).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "reranker failed; using un-reranked results");
                top
            }
        };

        // Apply search plugin post-processing (filtering, re-ranking, etc.)
        if !self.search_plugins.is_empty() {
            for plugin in &self.search_plugins {
                results = plugin.post_search(results);
            }
        }

        Ok(results)
    }

    /// Lexical-only retrieval (BM25 via the text-search backend) for the
    /// `Vectorless` retrieval mode. Skips query embedding and the dense-vector
    /// arm entirely — no embedding call, no vector store — then applies the same
    /// rerank + search-plugin post-processing as [`search`] so downstream
    /// behavior is consistent. The `doc_ids` filter is preserved.
    pub async fn search_lexical(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let effective_query_text = if self.search_plugins.is_empty() {
            query.text.clone()
        } else {
            let mut q = query.text.clone();
            for plugin in &self.search_plugins {
                q = plugin.pre_search(&q);
            }
            q
        };

        let text_query = SearchQuery {
            text: effective_query_text,
            top_k: self.config.top_k,
            workspace_ids: query.workspace_ids.clone(),
            unrestricted: query.unrestricted,
            query_images: Vec::new(),
            doc_ids: query.doc_ids.clone(),
        };
        let text_results = self.text_search.search(&text_query).await?;

        let top: Vec<SearchResult> = text_results
            .into_iter()
            .take(self.config.rerank_top_k)
            .collect();
        let mut results = match self.reranker.rerank(&query.text, top.clone()).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "reranker failed; using un-reranked results");
                top
            }
        };

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

    /// Clear BOTH indexes — vector store and text (BM25) index. Used by a global
    /// factory reset so no stale search state survives the Postgres wipe.
    pub async fn delete_all_indexes(&self) -> Result<()> {
        let (v_res, t_res) = tokio::join!(
            self.vector_store.delete_all(),
            self.text_search.delete_all()
        );
        v_res?;
        t_res?;
        Ok(())
    }

    /// Return statistics about the underlying vector store.
    pub async fn vector_store_stats(&self) -> Result<thairag_core::types::VectorStoreStats> {
        self.vector_store.collection_stats().await
    }

    pub(crate) fn rrf_merge(
        &self,
        vector_results: &[SearchResult],
        text_results: &[SearchResult],
        image_result_sets: &[Vec<SearchResult>],
    ) -> Vec<SearchResult> {
        let k = self.config.rrf_k as f32;
        let mut scores: HashMap<String, (f32, SearchResult)> = HashMap::new();

        // Capture each chunk's absolute dense cosine BEFORE fusion. RRF
        // normalization below scales the top hit to 1.0, erasing absolute
        // relevance; the no-context refusal gate reads this preserved value so it
        // works even when no reranker supplies absolute scores.
        let vector_cosine: HashMap<String, f32> = vector_results
            .iter()
            .map(|r| (r.chunk.chunk_id.to_string(), r.score))
            .collect();

        let mut fold = |results: &[SearchResult], weight: f32| {
            // A zero weight contributes nothing; skip so a disabled source never
            // injects zero-score entries into the merged set.
            if weight == 0.0 {
                return;
            }
            for (rank, result) in results.iter().enumerate() {
                let rrf_score = weight / (k + rank as f32 + 1.0);
                let key = result.chunk.chunk_id.to_string();
                scores
                    .entry(key)
                    .and_modify(|(s, _)| *s += rrf_score)
                    .or_insert((rrf_score, result.clone()));
            }
        };

        fold(vector_results, self.config.vector_weight);
        fold(text_results, self.config.text_weight);
        // Each image ranking (text→image and any image→image) folds with the
        // same configurable weight; the slice is empty (no-op) whenever CLIP
        // visual search is disabled.
        let image_weight = self.image_search.as_ref().map_or(0.0, |i| i.weight);
        for set in image_result_sets {
            fold(set, image_weight);
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

        // Re-attach the absolute cosine (survives the normalization above).
        for r in &mut merged {
            r.vector_score = vector_cosine.get(&r.chunk.chunk_id.to_string()).copied();
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

    struct MockImageEmbedding;
    #[async_trait]
    impl ImageEmbeddingModel for MockImageEmbedding {
        async fn embed_images(&self, images: &[Vec<u8>]) -> Result<Vec<Vec<f32>>> {
            Ok(images.iter().map(|_| vec![0.0; 4]).collect())
        }
        async fn embed_query_text(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.0; 4]).collect())
        }
        fn dimension(&self) -> usize {
            4
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
            vector_score: None,
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

    fn build_engine_with_image(
        rrf_k: usize,
        vw: f32,
        tw: f32,
        image_weight: f32,
    ) -> HybridSearchEngine {
        build_engine(rrf_k, vw, tw).with_image_search(
            Arc::new(MockImageEmbedding),
            Arc::new(MockVectorStore),
            image_weight,
        )
    }

    // ── RRF Merge Tests ──────────────────────────────────────────────

    #[test]
    fn rrf_both_empty() {
        let engine = build_engine(60, 0.5, 0.5);
        let merged = engine.rrf_merge(&[], &[], &[]);
        assert!(merged.is_empty());
    }

    #[test]
    fn rrf_vector_only() {
        let engine = build_engine(60, 1.0, 0.0);
        let id = "00000000-0000-0000-0000-000000000001";
        let vec_results = vec![make_result(id, 0.9)];
        let merged = engine.rrf_merge(&vec_results, &[], &[]);
        assert_eq!(merged.len(), 1);
        // Single result is normalized to 1.0
        assert!((merged[0].score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rrf_text_only() {
        let engine = build_engine(60, 0.0, 1.0);
        let id = "00000000-0000-0000-0000-000000000002";
        let text_results = vec![make_result(id, 0.8)];
        let merged = engine.rrf_merge(&[], &text_results, &[]);
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

        let merged = engine.rrf_merge(&vec_results, &text_results, &[]);
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

        let merged = engine.rrf_merge(&vec_results, &text_results, &[]);
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

        let merged = engine.rrf_merge(&vec_results, &text_results, &[]);
        assert_eq!(merged.len(), 2);
        // a (in both) should have a higher normalized score than b (text only)
        assert_eq!(merged[0].chunk.chunk_id.0.to_string(), id_a);
        assert!(merged[0].score > merged[1].score);
    }

    #[test]
    fn rrf_image_results_fuse_when_enabled() {
        // Image weight high; an image-only hit should still surface, and a
        // chunk appearing in image + vector lists should outrank an image-only
        // chunk — proving image_results actually fold into the merge.
        let engine = build_engine_with_image(60, 1.0, 0.0, 1.0);
        let shared_id = "00000000-0000-0000-0000-000000000007";
        let image_only_id = "00000000-0000-0000-0000-000000000008";

        let vec_results = vec![make_result(shared_id, 0.9)];
        let image_results = vec![make_result(shared_id, 0.8), make_result(image_only_id, 0.7)];

        let merged = engine.rrf_merge(&vec_results, &[], std::slice::from_ref(&image_results));
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].chunk.chunk_id.0.to_string(), shared_id);
        assert!(merged[0].score > merged[1].score);
    }

    #[test]
    fn rrf_image_weight_zero_when_disabled() {
        // No image_search attached → image_weight resolves to 0.0, so passing
        // image_results is a strict no-op (guarantee-or-drop: flag off = today's
        // behaviour, byte-for-byte).
        let engine = build_engine(60, 1.0, 0.0);
        let id = "00000000-0000-0000-0000-000000000009";
        let image_results = vec![make_result(id, 0.9)];

        let merged = engine.rrf_merge(&[], &[], std::slice::from_ref(&image_results));
        assert!(merged.is_empty());
    }

    #[test]
    fn rrf_multiple_image_sets_fuse() {
        // Two image rankings (e.g. text→image + image→image). A chunk that
        // ranks in both sets accrues two folds and should outrank a chunk that
        // appears in only one — proving each set folds independently.
        let engine = build_engine_with_image(60, 0.0, 0.0, 1.0);
        let in_both = "00000000-0000-0000-0000-00000000000c";
        let in_one = "00000000-0000-0000-0000-00000000000d";

        let text_to_image = vec![make_result(in_both, 0.9)];
        let image_to_image = vec![make_result(in_both, 0.8), make_result(in_one, 0.7)];
        let sets = vec![text_to_image, image_to_image];

        let merged = engine.rrf_merge(&[], &[], &sets);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].chunk.chunk_id.0.to_string(), in_both);
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

        let merged = engine.rrf_merge(&vec_results, &text_results, &[]);

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
