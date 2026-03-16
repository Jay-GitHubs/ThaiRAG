use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use thairag_core::error::Result;
use thairag_core::traits::PersonalMemoryStore;
use thairag_core::types::{MemoryId, PersonalMemory, UserId};

/// In-memory personal memory store for development and testing.
pub struct InMemoryPersonalMemoryStore {
    /// Stores memory alongside its embedding vector.
    entries: RwLock<HashMap<MemoryId, (PersonalMemory, Vec<f32>)>>,
}

impl Default for InMemoryPersonalMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryPersonalMemoryStore {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

#[async_trait]
impl PersonalMemoryStore for InMemoryPersonalMemoryStore {
    async fn store(&self, memory: &PersonalMemory, embedding: Vec<f32>) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.insert(memory.id, (memory.clone(), embedding));
        Ok(())
    }

    async fn search(
        &self,
        user_id: UserId,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<PersonalMemory>> {
        let entries = self.entries.read().unwrap();
        let mut scored: Vec<(f32, &PersonalMemory)> = entries
            .values()
            .filter(|(m, _)| m.user_id == user_id)
            .map(|(m, emb)| {
                let sim = cosine_similarity(query_embedding, emb);
                // Weight by relevance_score (decayed over time)
                (sim * m.relevance_score, m)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);

        Ok(scored.into_iter().map(|(_, m)| m.clone()).collect())
    }

    async fn delete(&self, id: MemoryId) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.remove(&id);
        Ok(())
    }

    async fn delete_all_for_user(&self, user_id: UserId) -> Result<()> {
        let mut entries = self.entries.write().unwrap();
        entries.retain(|_, (m, _)| m.user_id != user_id);
        Ok(())
    }

    async fn apply_decay(&self, decay_factor: f32, min_score: f32) -> Result<usize> {
        let mut entries = self.entries.write().unwrap();

        // Apply decay
        for (_, (m, _)) in entries.iter_mut() {
            m.relevance_score *= decay_factor;
        }

        // Remove entries below minimum relevance
        let before = entries.len();
        entries.retain(|_, (m, _)| m.relevance_score >= min_score);

        Ok(before - entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::PersonalMemoryType;

    fn make_memory(user_id: UserId, summary: &str, importance: f32) -> PersonalMemory {
        PersonalMemory {
            id: MemoryId::new(),
            user_id,
            memory_type: PersonalMemoryType::Fact,
            summary: summary.into(),
            topics: vec![],
            importance,
            created_at: 0,
            last_accessed_at: 0,
            relevance_score: 1.0,
        }
    }

    #[tokio::test]
    async fn store_and_search() {
        let store = InMemoryPersonalMemoryStore::new();
        let uid = UserId::new();
        let mem = make_memory(uid, "Works in HR", 0.8);
        let emb = vec![1.0, 0.0, 0.0];

        store.store(&mem, emb).await.unwrap();
        let results = store.search(uid, &[1.0, 0.0, 0.0], 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "Works in HR");
    }

    #[tokio::test]
    async fn search_filters_by_user() {
        let store = InMemoryPersonalMemoryStore::new();
        let uid1 = UserId::new();
        let uid2 = UserId::new();

        store
            .store(&make_memory(uid1, "User 1 fact", 0.8), vec![1.0, 0.0])
            .await
            .unwrap();
        store
            .store(&make_memory(uid2, "User 2 fact", 0.8), vec![1.0, 0.0])
            .await
            .unwrap();

        let results = store.search(uid1, &[1.0, 0.0], 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "User 1 fact");
    }

    #[tokio::test]
    async fn delete_all_for_user() {
        let store = InMemoryPersonalMemoryStore::new();
        let uid = UserId::new();

        store
            .store(&make_memory(uid, "Fact 1", 0.8), vec![1.0])
            .await
            .unwrap();
        store
            .store(&make_memory(uid, "Fact 2", 0.8), vec![0.5])
            .await
            .unwrap();

        store.delete_all_for_user(uid).await.unwrap();
        let results = store.search(uid, &[1.0], 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn apply_decay_prunes() {
        let store = InMemoryPersonalMemoryStore::new();
        let uid = UserId::new();

        let mut mem = make_memory(uid, "Old memory", 0.5);
        mem.relevance_score = 0.15; // just above 0.1
        store.store(&mem, vec![1.0]).await.unwrap();

        // Decay by 0.5 → 0.075, below min 0.1 → pruned
        let pruned = store.apply_decay(0.5, 0.1).await.unwrap();
        assert_eq!(pruned, 1);

        let results = store.search(uid, &[1.0], 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_returns_top_k() {
        let store = InMemoryPersonalMemoryStore::new();
        let uid = UserId::new();

        for i in 0..10 {
            let emb = vec![i as f32 / 10.0, 1.0 - (i as f32 / 10.0)];
            store
                .store(&make_memory(uid, &format!("Fact {i}"), 0.5), emb)
                .await
                .unwrap();
        }

        let results = store.search(uid, &[1.0, 0.0], 3).await.unwrap();
        assert_eq!(results.len(), 3);
    }
}
