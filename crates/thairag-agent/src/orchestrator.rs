use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;
use thairag_core::error::Result;
use thairag_core::permission::AccessScope;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, QueryIntent};

use crate::rag_engine::RagEngine;

/// Agent 1: Query Orchestrator.
/// Classifies intent, rewrites query, routes to RAG engine or direct LLM.
pub struct QueryOrchestrator {
    llm: Arc<dyn LlmProvider>,
    rag_engine: Arc<RagEngine>,
}

impl QueryOrchestrator {
    pub fn new(llm: Arc<dyn LlmProvider>, rag_engine: Arc<RagEngine>) -> Self {
        Self { llm, rag_engine }
    }

    /// Process a user query through the orchestration pipeline.
    pub async fn process(
        &self,
        messages: &[ChatMessage],
        scope: &AccessScope,
    ) -> Result<String> {
        let user_query = messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let intent = self.classify_intent(user_query).await;

        match intent {
            QueryIntent::DirectAnswer => {
                self.llm.generate(messages, None).await
            }
            QueryIntent::Retrieval => {
                let rewritten = self.rewrite_query(user_query).await;
                self.rag_engine.answer(&rewritten, messages, scope).await
            }
            QueryIntent::Clarification => {
                Ok("Could you please provide more details about your question?".to_string())
            }
        }
    }

    /// Process a user query and return a token stream.
    pub async fn process_stream(
        &self,
        messages: &[ChatMessage],
        scope: &AccessScope,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let user_query = messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let intent = self.classify_intent(user_query).await;

        match intent {
            QueryIntent::DirectAnswer => {
                self.llm.generate_stream(messages, None).await
            }
            QueryIntent::Retrieval => {
                let rewritten = self.rewrite_query(user_query).await;
                self.rag_engine.answer_stream(&rewritten, messages, scope).await
            }
            QueryIntent::Clarification => {
                let msg = "Could you please provide more details about your question?".to_string();
                Ok(Box::pin(tokio_stream::once(Ok(msg))))
            }
        }
    }

    async fn classify_intent(&self, _query: &str) -> QueryIntent {
        // Stub: always route to retrieval for now
        QueryIntent::Retrieval
    }

    async fn rewrite_query(&self, query: &str) -> String {
        // Stub: return query as-is
        query.to_string()
    }
}
