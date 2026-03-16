use std::sync::Arc;

use thairag_core::error::Result;
use thairag_core::traits::{EmbeddingModel, PersonalMemoryStore};
use thairag_core::types::{ChatMessage, PersonalMemory, UserId};
use tracing::{debug, warn};

/// Agent: Personal Memory Manager.
/// Handles storing, retrieving, and maintaining per-user memories in vector storage.
pub struct PersonalMemoryManager {
    embedding: Arc<dyn EmbeddingModel>,
    store: Arc<dyn PersonalMemoryStore>,
    top_k: usize,
    max_per_user: usize,
}

impl PersonalMemoryManager {
    pub fn new(
        embedding: Arc<dyn EmbeddingModel>,
        store: Arc<dyn PersonalMemoryStore>,
        top_k: usize,
        max_per_user: usize,
    ) -> Self {
        Self {
            embedding,
            store,
            top_k,
            max_per_user,
        }
    }

    /// Store extracted memories from context compaction into the vector database.
    pub async fn store_memories(&self, memories: &[PersonalMemory]) -> Result<()> {
        if memories.is_empty() {
            return Ok(());
        }

        // Build texts for embedding
        let texts: Vec<String> = memories
            .iter()
            .map(|m| {
                let topics = if m.topics.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", m.topics.join(", "))
                };
                format!("{}{}", m.summary, topics)
            })
            .collect();

        let embeddings = self.embedding.embed(&texts).await?;

        for (memory, embedding) in memories.iter().zip(embeddings.into_iter()) {
            if let Err(e) = self.store.store(memory, embedding).await {
                warn!(
                    memory_id = %memory.id,
                    error = %e,
                    "Failed to store personal memory"
                );
            }
        }

        debug!(
            count = memories.len(),
            user_id = %memories[0].user_id,
            "Personal memories stored"
        );
        Ok(())
    }

    /// Retrieve relevant personal memories for a user's current query.
    pub async fn retrieve(&self, user_id: UserId, query: &str) -> Result<Vec<PersonalMemory>> {
        let query_embedding = self.embedding.embed(&[query.to_string()]).await?;
        let embedding = query_embedding.into_iter().next().unwrap_or_default();

        let memories = self.store.search(user_id, &embedding, self.top_k).await?;

        if !memories.is_empty() {
            debug!(
                user_id = %user_id,
                count = memories.len(),
                "Retrieved personal memories for context"
            );
        }

        Ok(memories)
    }

    /// Build a system message from retrieved personal memories for injection into the pipeline.
    pub fn build_memory_context(memories: &[PersonalMemory]) -> Option<ChatMessage> {
        if memories.is_empty() {
            return None;
        }

        let mut context =
            String::from("Personal context about this user (from past conversations):\n");
        for (i, m) in memories.iter().enumerate() {
            let type_label = match m.memory_type {
                thairag_core::types::PersonalMemoryType::Preference => "Preference",
                thairag_core::types::PersonalMemoryType::Fact => "Fact",
                thairag_core::types::PersonalMemoryType::Decision => "Decision",
                thairag_core::types::PersonalMemoryType::Conversation => "Context",
                thairag_core::types::PersonalMemoryType::Correction => "Correction",
            };
            let topics = if m.topics.is_empty() {
                String::new()
            } else {
                format!(" [{}]", m.topics.join(", "))
            };
            context.push_str(&format!(
                "{}. [{}] {}{}\n",
                i + 1,
                type_label,
                m.summary,
                topics
            ));
        }
        context.push_str(
            "\nUse this personal context to tailor your response. \
             Reference relevant past context when helpful, but don't force it.",
        );

        Some(ChatMessage {
            role: "system".into(),
            content: context,
        })
    }

    /// Delete all personal memories for a user (privacy/GDPR compliance).
    pub async fn clear_user_memories(&self, user_id: UserId) -> Result<()> {
        self.store.delete_all_for_user(user_id).await?;
        debug!(user_id = %user_id, "Cleared all personal memories");
        Ok(())
    }

    /// Getters for configuration.
    pub fn max_per_user(&self) -> usize {
        self.max_per_user
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::{MemoryId, PersonalMemoryType};

    #[test]
    fn build_memory_context_empty() {
        assert!(PersonalMemoryManager::build_memory_context(&[]).is_none());
    }

    #[test]
    fn build_memory_context_single() {
        let memories = vec![PersonalMemory {
            id: MemoryId::new(),
            user_id: UserId::new(),
            memory_type: PersonalMemoryType::Preference,
            summary: "Prefers bullet points".into(),
            topics: vec!["format".into()],
            importance: 0.9,
            created_at: 0,
            last_accessed_at: 0,
            relevance_score: 1.0,
        }];
        let msg = PersonalMemoryManager::build_memory_context(&memories).unwrap();
        assert_eq!(msg.role, "system");
        assert!(msg.content.contains("Preference"));
        assert!(msg.content.contains("bullet points"));
        assert!(msg.content.contains("[format]"));
    }

    #[test]
    fn build_memory_context_multiple_types() {
        let uid = UserId::new();
        let memories = vec![
            PersonalMemory {
                id: MemoryId::new(),
                user_id: uid,
                memory_type: PersonalMemoryType::Fact,
                summary: "Works in HR department".into(),
                topics: vec!["role".into()],
                importance: 0.8,
                created_at: 0,
                last_accessed_at: 0,
                relevance_score: 1.0,
            },
            PersonalMemory {
                id: MemoryId::new(),
                user_id: uid,
                memory_type: PersonalMemoryType::Decision,
                summary: "Team chose PostgreSQL for the project".into(),
                topics: vec!["database".into(), "architecture".into()],
                importance: 0.7,
                created_at: 0,
                last_accessed_at: 0,
                relevance_score: 0.9,
            },
        ];
        let msg = PersonalMemoryManager::build_memory_context(&memories).unwrap();
        assert!(msg.content.contains("Fact"));
        assert!(msg.content.contains("Decision"));
        assert!(msg.content.contains("PostgreSQL"));
    }
}
