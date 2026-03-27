use std::sync::Arc;

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::{EmbeddingCache, EmbeddingModel};

/// An `EmbeddingModel` wrapper that caches embeddings. Cache hits bypass
/// the underlying model entirely; misses are embedded and then stored.
pub struct CachedEmbeddingModel {
    inner: Arc<dyn EmbeddingModel>,
    cache: Arc<dyn EmbeddingCache>,
}

impl CachedEmbeddingModel {
    pub fn new(inner: Arc<dyn EmbeddingModel>, cache: Arc<dyn EmbeddingCache>) -> Self {
        Self { inner, cache }
    }
}

#[async_trait]
impl EmbeddingModel for CachedEmbeddingModel {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Check cache for all texts
        let cached = self.cache.get_many(texts).await;

        // Partition into hits and misses
        let mut results: Vec<Option<Vec<f32>>> = cached;
        let miss_indices: Vec<usize> = results
            .iter()
            .enumerate()
            .filter_map(|(i, v)| if v.is_none() { Some(i) } else { None })
            .collect();

        if miss_indices.is_empty() {
            // All hits
            return Ok(results.into_iter().map(|v| v.unwrap()).collect());
        }

        // Embed only the misses
        let miss_texts: Vec<String> = miss_indices.iter().map(|&i| texts[i].clone()).collect();
        let miss_embeddings = self.inner.embed(&miss_texts).await?;

        // Store misses in cache and fill results
        let mut cache_pairs = Vec::with_capacity(miss_indices.len());
        for (mi, &idx) in miss_indices.iter().enumerate() {
            let emb = miss_embeddings[mi].clone();
            cache_pairs.push((texts[idx].clone(), emb.clone()));
            results[idx] = Some(emb);
        }
        self.cache.put_many(cache_pairs).await;

        if !miss_indices.is_empty() {
            tracing::debug!(
                hits = texts.len() - miss_indices.len(),
                misses = miss_indices.len(),
                "Embedding cache"
            );
        }

        Ok(results.into_iter().map(|v| v.unwrap()).collect())
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }
}
