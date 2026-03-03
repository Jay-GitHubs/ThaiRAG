use std::sync::Arc;

use thairag_core::error::Result;
use thairag_core::permission::AccessScope;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse, LlmStreamResponse, SearchQuery};
use thairag_search::HybridSearchEngine;

/// Agent 2: RAG Engine.
/// Performs hybrid search with access scope → post-retrieval guardrail →
/// builds context-augmented prompt → generates answer.
pub struct RagEngine {
    llm: Arc<dyn LlmProvider>,
    search: Arc<HybridSearchEngine>,
}

impl RagEngine {
    pub fn new(llm: Arc<dyn LlmProvider>, search: Arc<HybridSearchEngine>) -> Self {
        Self { llm, search }
    }

    pub async fn answer(
        &self,
        query: &str,
        messages: &[ChatMessage],
        scope: &AccessScope,
    ) -> Result<LlmResponse> {
        // Build search query with access scope
        let search_query = SearchQuery {
            text: query.to_string(),
            top_k: 5,
            workspace_ids: scope.workspace_ids.clone(),
        };

        // Hybrid search
        let results = self.search.search(&search_query).await?;

        // Build context from search results
        let context = results
            .iter()
            .enumerate()
            .map(|(i, r)| format!("[{}] {}", i + 1, r.chunk.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        // Build augmented prompt
        let system_prompt = format!(
            "You are ThaiRAG, an AI assistant specialized in Thai language documents.\n\
             Use the following context to answer the user's question.\n\
             If the context doesn't contain relevant information, say so.\n\n\
             Context:\n{context}"
        );

        let mut augmented_messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        }];
        augmented_messages.extend_from_slice(messages);

        self.llm.generate(&augmented_messages, None).await
    }

    pub async fn answer_stream(
        &self,
        query: &str,
        messages: &[ChatMessage],
        scope: &AccessScope,
    ) -> Result<LlmStreamResponse> {
        // Same search + augmentation as answer(), then stream the LLM generation
        let search_query = SearchQuery {
            text: query.to_string(),
            top_k: 5,
            workspace_ids: scope.workspace_ids.clone(),
        };

        let results = self.search.search(&search_query).await?;

        let context = results
            .iter()
            .enumerate()
            .map(|(i, r)| format!("[{}] {}", i + 1, r.chunk.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let system_prompt = format!(
            "You are ThaiRAG, an AI assistant specialized in Thai language documents.\n\
             Use the following context to answer the user's question.\n\
             If the context doesn't contain relevant information, say so.\n\n\
             Context:\n{context}"
        );

        let mut augmented_messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        }];
        augmented_messages.extend_from_slice(messages);

        self.llm.generate_stream(&augmented_messages, None).await
    }
}
