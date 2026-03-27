use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use async_trait::async_trait;
use thairag_core::traits::EmbeddingCache;

struct CacheEntry {
    embedding: Vec<f32>,
    last_accessed: Instant,
}

/// In-memory embedding cache with TTL-based expiration and max-entries cap.
pub struct InMemoryEmbeddingCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    max_entries: usize,
    ttl: std::time::Duration,
}

impl InMemoryEmbeddingCache {
    pub fn new(max_entries: usize, ttl_secs: u64) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            max_entries,
            ttl: std::time::Duration::from_secs(ttl_secs),
        }
    }

    /// Evict expired entries and, if still over capacity, evict oldest-accessed entries.
    fn evict_if_needed(
        entries: &mut HashMap<String, CacheEntry>,
        max: usize,
        ttl: std::time::Duration,
    ) {
        let now = Instant::now();

        // Remove expired entries
        entries.retain(|_, e| now.duration_since(e.last_accessed) < ttl);

        // If still over max, remove oldest entries
        if entries.len() > max {
            let mut by_age: Vec<(String, Instant)> = entries
                .iter()
                .map(|(k, e)| (k.clone(), e.last_accessed))
                .collect();
            by_age.sort_by_key(|(_, t)| *t);

            let to_remove = entries.len() - max;
            for (key, _) in by_age.into_iter().take(to_remove) {
                entries.remove(&key);
            }
        }
    }
}

#[async_trait]
impl EmbeddingCache for InMemoryEmbeddingCache {
    async fn get(&self, text: &str) -> Option<Vec<f32>> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(text) {
            if entry.last_accessed.elapsed() < self.ttl {
                entry.last_accessed = Instant::now();
                return Some(entry.embedding.clone());
            }
            // Expired
            entries.remove(text);
        }
        None
    }

    async fn get_many(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        let mut entries = self.entries.lock().unwrap();
        let now = Instant::now();
        texts
            .iter()
            .map(|text| {
                if let Some(entry) = entries.get_mut(text.as_str()) {
                    if now.duration_since(entry.last_accessed) < self.ttl {
                        entry.last_accessed = now;
                        return Some(entry.embedding.clone());
                    }
                    entries.remove(text.as_str());
                }
                None
            })
            .collect()
    }

    async fn put(&self, text: &str, embedding: Vec<f32>) {
        let mut entries = self.entries.lock().unwrap();
        entries.insert(
            text.to_string(),
            CacheEntry {
                embedding,
                last_accessed: Instant::now(),
            },
        );
        Self::evict_if_needed(&mut entries, self.max_entries, self.ttl);
    }

    async fn put_many(&self, pairs: Vec<(String, Vec<f32>)>) {
        let mut entries = self.entries.lock().unwrap();
        let now = Instant::now();
        for (text, embedding) in pairs {
            entries.insert(
                text,
                CacheEntry {
                    embedding,
                    last_accessed: now,
                },
            );
        }
        Self::evict_if_needed(&mut entries, self.max_entries, self.ttl);
    }

    async fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

/// A no-op cache that never stores or returns anything.
/// Used when embedding caching is disabled.
pub struct NoopEmbeddingCache;

#[async_trait]
impl EmbeddingCache for NoopEmbeddingCache {
    async fn get(&self, _text: &str) -> Option<Vec<f32>> {
        None
    }
    async fn get_many(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        vec![None; texts.len()]
    }
    async fn put(&self, _text: &str, _embedding: Vec<f32>) {}
    async fn put_many(&self, _pairs: Vec<(String, Vec<f32>)>) {}
    async fn len(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get() {
        let cache = InMemoryEmbeddingCache::new(100, 3600);
        cache.put("hello", vec![1.0, 2.0, 3.0]).await;
        let result = cache.get("hello").await;
        assert_eq!(result, Some(vec![1.0, 2.0, 3.0]));
    }

    #[tokio::test]
    async fn miss_returns_none() {
        let cache = InMemoryEmbeddingCache::new(100, 3600);
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn get_many_mixed() {
        let cache = InMemoryEmbeddingCache::new(100, 3600);
        cache.put("a", vec![1.0]).await;
        cache.put("c", vec![3.0]).await;

        let results = cache.get_many(&["a".into(), "b".into(), "c".into()]).await;
        assert_eq!(results[0], Some(vec![1.0]));
        assert_eq!(results[1], None);
        assert_eq!(results[2], Some(vec![3.0]));
    }

    #[tokio::test]
    async fn evicts_when_over_max() {
        let cache = InMemoryEmbeddingCache::new(2, 3600);
        cache.put("a", vec![1.0]).await;
        cache.put("b", vec![2.0]).await;
        cache.put("c", vec![3.0]).await;

        assert_eq!(cache.len().await, 2);
        // 'a' was oldest, should be evicted
        assert!(cache.get("a").await.is_none());
        assert!(cache.get("c").await.is_some());
    }

    #[tokio::test]
    async fn put_many_works() {
        let cache = InMemoryEmbeddingCache::new(100, 3600);
        cache
            .put_many(vec![("x".into(), vec![10.0]), ("y".into(), vec![20.0])])
            .await;
        assert_eq!(cache.len().await, 2);
        assert_eq!(cache.get("x").await, Some(vec![10.0]));
    }

    #[tokio::test]
    async fn noop_cache() {
        let cache = NoopEmbeddingCache;
        cache.put("hello", vec![1.0]).await;
        assert!(cache.get("hello").await.is_none());
        assert_eq!(cache.len().await, 0);
    }
}
