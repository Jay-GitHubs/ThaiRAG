use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;
use thairag_core::error::Result;
use thairag_core::permission::AccessScope;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, QueryIntent};
use thairag_thai::ThaiNormalizer;

use crate::rag_engine::RagEngine;

// ── Filler prefixes to strip (checked after lowercasing) ─────────────

const EN_FILLERS: &[&str] = &[
    "could you please",
    "can you please",
    "please tell me about",
    "please explain",
    "please help me with",
    "please describe",
    "please show me",
    "tell me about",
    "explain to me",
    "i want to know about",
    "i would like to know about",
    "i'd like to know about",
    "can you tell me",
    "could you tell me",
    "can you explain",
    "could you explain",
    "what is",
    "what are",
    "please",
    "can you",
    "could you",
];

const TH_FILLERS: &[&str] = &[
    "ช่วยอธิบายเรื่อง",
    "ช่วยบอกเกี่ยวกับ",
    "ช่วยอธิบาย",
    "ช่วยบอก",
    "ช่วย",
    "อยากรู้เรื่อง",
    "อยากรู้เกี่ยวกับ",
    "อยากรู้ว่า",
    "อยากรู้",
    "อยากทราบ",
    "กรุณา",
    "ขอถามเรื่อง",
    "ขอถามว่า",
    "ขอถาม",
];

// ── Conversational patterns that trigger LLM rewriting ───────────────

const CONVERSATIONAL_PATTERNS: &[&str] = &[
    "difference between",
    "differences between",
    "compare",
    "vs",
    "versus",
    "pros and cons",
    "advantages and disadvantages",
    "how does",
    "how do",
    "why does",
    "why do",
    "what happens when",
    "เปรียบเทียบ",
    "แตกต่าง",
    "ข้อดีข้อเสีย",
];

const LLM_REWRITE_LENGTH_THRESHOLD: usize = 80;

// ── Free functions ───────────────────────────────────────────────────

/// Stage 1: heuristic normalization (zero latency).
/// - Normalize whitespace via ThaiNormalizer
/// - Strip trailing punctuation
/// - Remove one filler prefix
fn heuristic_normalize(query: &str) -> String {
    let normalizer = ThaiNormalizer::new();
    let normalized = normalizer.normalize(query);

    // Strip trailing punctuation
    let trimmed = normalized
        .trim_end_matches(|c: char| matches!(c, '?' | '!' | '.' | '…'));
    let trimmed = trimmed.trim();

    // Remove one filler prefix (longest match first — lists are ordered long→short)
    let lower = trimmed.to_lowercase();

    for filler in TH_FILLERS {
        if lower.starts_with(filler) {
            let rest = &trimmed[filler.len()..].trim_start();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }

    for filler in EN_FILLERS {
        if lower.starts_with(filler) {
            let rest = trimmed[filler.len()..].trim_start();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }

    trimmed.to_string()
}

/// Check whether the query needs LLM-based rewriting.
fn needs_llm_rewrite(normalized: &str) -> bool {
    if normalized.len() > LLM_REWRITE_LENGTH_THRESHOLD {
        return true;
    }
    let lower = normalized.to_lowercase();
    CONVERSATIONAL_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Classify intent from the raw user query (free fn — no `self`).
fn classify_intent(query: &str) -> QueryIntent {
    let trimmed = query.trim();
    let lower = trimmed.to_lowercase();

    // Greeting patterns (English + Thai) — checked BEFORE length gate
    const GREETINGS: &[&str] = &[
        "hi", "hello", "hey", "howdy", "good morning", "good afternoon",
        "good evening", "yo", "sup", "what's up", "whats up",
        "สวัสดี", "หวัดดี", "ดีครับ", "ดีค่ะ", "สวัสดีครับ", "สวัสดีค่ะ",
    ];

    for pat in GREETINGS {
        if lower == *pat
            || lower.starts_with(&format!("{pat} "))
            || lower.starts_with(&format!("{pat},"))
            || lower.starts_with(&format!("{pat}!"))
        {
            return QueryIntent::DirectAnswer;
        }
    }

    // Empty or very short queries → ask for clarification
    if trimmed.len() <= 2 {
        return QueryIntent::Clarification;
    }

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

// ── QueryOrchestrator ────────────────────────────────────────────────

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

        let intent = classify_intent(user_query);

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

        let intent = classify_intent(user_query);

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

    /// Two-stage query rewriting:
    /// 1. Heuristic normalization (always)
    /// 2. LLM rewriting (conditional — long/conversational queries)
    async fn rewrite_query(&self, query: &str) -> String {
        let normalized = heuristic_normalize(query);

        if !needs_llm_rewrite(&normalized) {
            return normalized;
        }

        // Stage 2: LLM rewrite
        let system = ChatMessage {
            role: "system".to_string(),
            content: "Rewrite the user's query into a concise, keyword-rich search query. \
                      Output ONLY the rewritten query, nothing else. \
                      Preserve the original language (Thai or English)."
                .to_string(),
        };
        let user = ChatMessage {
            role: "user".to_string(),
            content: normalized.clone(),
        };

        match self.llm.generate(&[system, user], Some(100)).await {
            Ok(rewritten) => {
                let rewritten = rewritten.trim().to_string();
                if rewritten.is_empty() {
                    normalized
                } else {
                    rewritten
                }
            }
            Err(_) => normalized, // fallback to heuristic on LLM error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use thairag_core::error::Result;
    use thairag_core::types::ChatMessage;

    // ── Mock LLM Provider ────────────────────────────────────────────

    struct MockLlmProvider {
        response: String,
    }

    impl MockLlmProvider {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_string(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlmProvider {
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<String> {
            Ok(self.response.clone())
        }

        fn model_name(&self) -> &str {
            "mock"
        }
    }

    // ── Intent Classifier Tests ──────────────────────────────────────

    #[test]
    fn classify_greeting_en() {
        assert_eq!(classify_intent("hello"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("Hi there"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("hey"), QueryIntent::DirectAnswer);
    }

    #[test]
    fn classify_greeting_th() {
        assert_eq!(classify_intent("สวัสดี"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("หวัดดี"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("ดีครับ"), QueryIntent::DirectAnswer);
    }

    #[test]
    fn classify_thanks() {
        assert_eq!(classify_intent("thanks"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("Thank you so much"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("ขอบคุณครับ"), QueryIntent::DirectAnswer);
    }

    #[test]
    fn classify_meta() {
        assert_eq!(classify_intent("who are you?"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("What can you do"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("คุณเป็นใคร"), QueryIntent::DirectAnswer);
    }

    #[test]
    fn classify_empty_and_short() {
        assert_eq!(classify_intent(""), QueryIntent::Clarification);
        assert_eq!(classify_intent("  "), QueryIntent::Clarification);
        assert_eq!(classify_intent("ok"), QueryIntent::Clarification);
    }

    #[test]
    fn classify_retrieval() {
        assert_eq!(
            classify_intent("How do I configure logging in Rust?"),
            QueryIntent::Retrieval
        );
        assert_eq!(
            classify_intent("วิธีการตั้งค่า database"),
            QueryIntent::Retrieval
        );
    }

    #[test]
    fn classify_case_insensitive() {
        assert_eq!(classify_intent("HELLO"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("Hello!"), QueryIntent::DirectAnswer);
        assert_eq!(classify_intent("THANKS"), QueryIntent::DirectAnswer);
    }

    #[test]
    fn classify_hi_is_greeting_not_clarification() {
        // Regression: "hi" (2 bytes) must match greeting BEFORE length check
        assert_eq!(classify_intent("hi"), QueryIntent::DirectAnswer);
    }

    #[test]
    fn classify_yo_is_greeting() {
        assert_eq!(classify_intent("yo"), QueryIntent::DirectAnswer);
    }

    // ── Heuristic Normalize Tests ────────────────────────────────────

    #[test]
    fn normalize_strips_trailing_punctuation() {
        assert_eq!(heuristic_normalize("what is Rust?"), "Rust");
        assert_eq!(heuristic_normalize("hello!"), "hello");
        assert_eq!(heuristic_normalize("testing..."), "testing");
    }

    #[test]
    fn normalize_removes_en_filler() {
        assert_eq!(heuristic_normalize("please tell me about Rust"), "Rust");
        assert_eq!(heuristic_normalize("Can you explain tokio?"), "tokio");
        assert_eq!(
            heuristic_normalize("I want to know about async"),
            "async"
        );
    }

    #[test]
    fn normalize_removes_th_filler() {
        assert_eq!(heuristic_normalize("ช่วยอธิบาย Rust"), "Rust");
        assert_eq!(heuristic_normalize("อยากรู้เรื่อง async"), "async");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(
            heuristic_normalize("  hello   world  "),
            "hello world"
        );
    }

    #[test]
    fn normalize_preserves_meaningful_query() {
        assert_eq!(heuristic_normalize("Rust async runtime"), "Rust async runtime");
    }

    #[test]
    fn normalize_filler_only_returns_filler() {
        // If removing filler leaves nothing, keep the original
        assert_eq!(heuristic_normalize("please"), "please");
    }

    #[test]
    fn normalize_empty() {
        assert_eq!(heuristic_normalize(""), "");
        assert_eq!(heuristic_normalize("   "), "");
    }

    #[test]
    fn normalize_longest_filler_wins() {
        // "please tell me about" is longer than "please", so the full prefix is removed
        assert_eq!(
            heuristic_normalize("please tell me about generics"),
            "generics"
        );
    }

    // ── needs_llm_rewrite Tests ──────────────────────────────────────

    #[test]
    fn llm_rewrite_short_simple() {
        assert!(!needs_llm_rewrite("Rust async"));
    }

    #[test]
    fn llm_rewrite_long_query() {
        let long = "a".repeat(81);
        assert!(needs_llm_rewrite(&long));
    }

    #[test]
    fn llm_rewrite_conversational_pattern() {
        assert!(needs_llm_rewrite("difference between tokio and async-std"));
        assert!(needs_llm_rewrite("เปรียบเทียบ Rust กับ Go"));
    }

    #[test]
    fn llm_rewrite_boundary() {
        let exactly_80 = "a".repeat(80);
        assert!(!needs_llm_rewrite(&exactly_80));
    }

    // ── Full rewrite_query Tests (async, with mock LLM) ──────────────

    use thairag_core::traits::{EmbeddingModel, Reranker, TextSearch, VectorStore};
    use thairag_core::types::{DocId, DocumentChunk, SearchQuery, SearchResult};
    use thairag_config::schema::SearchConfig;

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
        async fn delete_by_doc(&self, _doc_id: DocId) -> Result<()> {
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
        async fn delete_by_doc(&self, _doc_id: DocId) -> Result<()> {
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

    fn build_test_orchestrator(llm_response: &str) -> QueryOrchestrator {
        let llm: Arc<dyn LlmProvider> = Arc::new(MockLlmProvider::new(llm_response));
        let search_config = SearchConfig {
            top_k: 5,
            rerank_top_k: 3,
            rrf_k: 60,
            vector_weight: 0.5,
            text_weight: 0.5,
        };
        let engine = thairag_search::HybridSearchEngine::new(
            Arc::new(MockEmbedding),
            Arc::new(MockVectorStore),
            Arc::new(MockTextSearch),
            Arc::new(MockReranker),
            search_config,
        );
        let rag = Arc::new(RagEngine::new(Arc::clone(&llm), Arc::new(engine)));
        QueryOrchestrator::new(llm, rag)
    }

    #[tokio::test]
    async fn rewrite_short_query_heuristic_only() {
        let orch = build_test_orchestrator("should not be called");
        // Short query → heuristic only, LLM not invoked
        let result = orch.rewrite_query("please tell me about Rust").await;
        assert_eq!(result, "Rust");
    }

    #[tokio::test]
    async fn rewrite_long_query_uses_llm() {
        let long_query = format!("please explain {}", "very ".repeat(20));
        let orch = build_test_orchestrator("concise search terms");
        let result = orch.rewrite_query(&long_query).await;
        assert_eq!(result, "concise search terms");
    }

    #[tokio::test]
    async fn rewrite_conversational_uses_llm() {
        let orch = build_test_orchestrator("tokio vs async-std comparison");
        let result = orch
            .rewrite_query("what is the difference between tokio and async-std")
            .await;
        assert_eq!(result, "tokio vs async-std comparison");
    }

    #[tokio::test]
    async fn rewrite_llm_error_falls_back() {
        // MockLlmProvider always succeeds, so test the empty-response fallback
        let orch = build_test_orchestrator("   ");
        let result = orch
            .rewrite_query("what is the difference between tokio and async-std")
            .await;
        // LLM returned whitespace → falls back to heuristic
        assert_eq!(result, "the difference between tokio and async-std");
    }
}
