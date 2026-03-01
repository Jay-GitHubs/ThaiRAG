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

        let intent = self.classify_intent(user_query);

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

        let intent = self.classify_intent(user_query);

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

    fn classify_intent(&self, query: &str) -> QueryIntent {
        let trimmed = query.trim();

        // Empty or very short queries → ask for clarification
        if trimmed.len() <= 2 {
            return QueryIntent::Clarification;
        }

        let lower = trimmed.to_lowercase();

        // Greeting patterns (English + Thai)
        const GREETINGS: &[&str] = &[
            "hi", "hello", "hey", "howdy", "good morning", "good afternoon",
            "good evening", "yo", "sup", "what's up", "whats up",
            "สวัสดี", "หวัดดี", "ดีครับ", "ดีค่ะ", "สวัสดีครับ", "สวัสดีค่ะ",
        ];

        // Thanks patterns
        const THANKS: &[&str] = &[
            "thank", "thanks", "thx", "ty",
            "ขอบคุณ", "ขอบใจ",
        ];

        // Meta questions about the bot itself
        const META: &[&str] = &[
            "who are you", "what are you", "what can you do",
            "คุณเป็นใคร", "คุณทำอะไรได้",
        ];

        for pat in GREETINGS {
            if lower == *pat || lower.starts_with(&format!("{pat} "))
                || lower.starts_with(&format!("{pat},"))
                || lower.starts_with(&format!("{pat}!"))
            {
                return QueryIntent::DirectAnswer;
            }
        }

        for pat in THANKS {
            if lower.contains(pat) {
                return QueryIntent::DirectAnswer;
            }
        }

        for pat in META {
            if lower.contains(pat) {
                return QueryIntent::DirectAnswer;
            }
        }

        QueryIntent::Retrieval
    }

    async fn rewrite_query(&self, query: &str) -> String {
        // Stub: return query as-is
        query.to_string()
    }
}
