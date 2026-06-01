//! Extract-then-answer post-step (Thai answer-quality experiment).
//!
//! The Thai failure mode is "generic-essay drift": the model writes fluent Thai
//! prose that omits the specific fact present in the retrieved context. This
//! module splits answering into two LLM calls:
//!
//!   1. **Extract** — copy, verbatim, the span(s) of the context that directly
//!      answer the query. If the context does not contain an answer, emit the
//!      sentinel `NONE`.
//!   2. **Answer** — compose the final reply using ONLY the extracted span(s),
//!      which keeps the model anchored to retrieved facts instead of drifting
//!      into generic prose.
//!
//! When extraction yields `NONE` (or empty), [`StructuredExtractor::answer`]
//! returns `Ok(None)` so the caller falls back to the normal response
//! generator rather than fabricating an answer.

use std::sync::Arc;

use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, LlmResponse};
use tracing::debug;

use crate::context_curator::CuratedContext;
use crate::query_analyzer::{QueryAnalysis, QueryLanguage};

/// Sentinel the extract step emits when the context contains no answer.
const NONE_SENTINEL: &str = "NONE";

const DEFAULT_EXTRACT: &str = "You are a precise information extractor. Given a user question and \
retrieved context, copy — VERBATIM — only the sentence(s) or phrase(s) from the context that \
directly answer the question.\n\n\
Rules:\n\
- Copy text EXACTLY as it appears. Do not paraphrase, translate, summarize, or add words.\n\
- Include only spans that bear on the question; omit everything else.\n\
- If the context does not contain an answer, output exactly: NONE\n\
- Output the extracted span(s) only — no preamble, no explanation.";

const DEFAULT_ANSWER: &str = "You are ThaiRAG, an AI assistant. Answer the user's question using \
ONLY the extracted facts below. Do NOT add information beyond these facts.\n\n\
{{language_instruction}}\n\n\
If the extracted facts do not fully answer the question, say so honestly rather than guessing. \
NEVER reference internal markup or index numbers.\n\n\
Extracted facts:\n{{extracted}}";

/// Two-step extract-then-answer generator.
pub struct StructuredExtractor {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl StructuredExtractor {
    pub fn new(llm: Arc<dyn LlmProvider>, max_tokens: u32) -> Self {
        Self {
            llm,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            prompts,
        }
    }

    /// Run extract-then-answer. Returns `Ok(None)` when no answer-bearing span
    /// is found in the context, signalling the caller to fall back to the
    /// normal response generator.
    pub async fn answer(
        &self,
        analysis: &QueryAnalysis,
        user_query: &str,
        context: &CuratedContext,
        max_tokens: Option<u32>,
    ) -> Result<Option<LlmResponse>> {
        if context.chunks.is_empty() {
            return Ok(None);
        }

        let extracted = self.extract(user_query, context).await?;
        let trimmed = extracted.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(NONE_SENTINEL) {
            debug!("StructuredExtractor: no answer-bearing span found, falling back");
            return Ok(None);
        }

        let response = self
            .compose(analysis, user_query, trimmed, max_tokens)
            .await?;
        Ok(Some(response))
    }

    /// Step 1: copy verbatim the answer-bearing span(s) from the context.
    async fn extract(&self, user_query: &str, context: &CuratedContext) -> Result<String> {
        let context_text = render_context(context);
        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.structured_extraction.extract",
                DEFAULT_EXTRACT,
                &[],
            ),
            images: vec![],
        };
        let user = ChatMessage {
            role: "user".into(),
            content: format!("Question: {user_query}\n\nContext:\n{context_text}"),
            images: vec![],
        };
        let resp = self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await?;
        Ok(resp.content.trim().to_string())
    }

    /// Step 2: compose the final answer using only the extracted span(s).
    async fn compose(
        &self,
        analysis: &QueryAnalysis,
        user_query: &str,
        extracted: &str,
        max_tokens: Option<u32>,
    ) -> Result<LlmResponse> {
        let language_instruction = match analysis.language {
            QueryLanguage::Thai => "Respond in Thai (ภาษาไทย). Use formal register.",
            QueryLanguage::English => "Respond in English.",
            QueryLanguage::Mixed => "Respond in the same language mix the user used.",
        };
        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.structured_extraction.answer",
                DEFAULT_ANSWER,
                &[
                    ("language_instruction", language_instruction),
                    ("extracted", extracted),
                ],
            ),
            images: vec![],
        };
        let user = ChatMessage {
            role: "user".into(),
            content: user_query.to_string(),
            images: vec![],
        };
        self.llm.generate(&[system, user], max_tokens).await
    }
}

/// Render curated chunks into a delimited block (mirrors the response
/// generator's anti-injection framing so extraction sees the same text).
fn render_context(context: &CuratedContext) -> String {
    context
        .chunks
        .iter()
        .map(|c| {
            if let Some(ref title) = c.source_doc_title {
                format!(
                    "<chunk index=\"{}\" source=\"{}\">\n{}\n</chunk>",
                    c.index, title, c.content
                )
            } else {
                format!("<chunk index=\"{}\">\n{}\n</chunk>", c.index, c.content)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use thairag_core::types::{ChunkId, DocId};

    use super::*;
    use crate::context_curator::CuratedChunk;
    use crate::query_analyzer::fallback_analyze;

    /// Mock LLM that returns scripted responses in FIFO order.
    struct ScriptedLlm {
        responses: Mutex<Vec<String>>,
    }

    impl ScriptedLlm {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Mutex::new(responses.iter().rev().map(|s| s.to_string()).collect()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedLlm {
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<LlmResponse> {
            let content = self
                .responses
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| "EXHAUSTED".to_string());
            Ok(LlmResponse {
                content,
                usage: Default::default(),
            })
        }
        fn model_name(&self) -> &str {
            "scripted"
        }
    }

    fn ctx(contents: &[&str]) -> CuratedContext {
        let chunks = contents
            .iter()
            .enumerate()
            .map(|(i, c)| CuratedChunk {
                index: i + 1,
                content: c.to_string(),
                relevance_score: 0.8,
                source_doc_id: DocId::new(),
                source_chunk_id: ChunkId::new(),
                source_doc_title: None,
            })
            .collect();
        CuratedContext {
            chunks,
            total_tokens_est: 0,
        }
    }

    fn extractor(responses: Vec<&str>) -> StructuredExtractor {
        StructuredExtractor::new(Arc::new(ScriptedLlm::new(responses)), 256)
    }

    #[tokio::test]
    async fn returns_none_when_context_empty() {
        let ex = extractor(vec!["should not be called"]);
        let out = ex
            .answer(&fallback_analyze("q"), "q", &ctx(&[]), None)
            .await
            .unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_extraction_is_none_sentinel() {
        let ex = extractor(vec!["NONE"]);
        let out = ex
            .answer(
                &fallback_analyze("q"),
                "q",
                &ctx(&["irrelevant text"]),
                None,
            )
            .await
            .unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn none_sentinel_is_case_insensitive() {
        let ex = extractor(vec!["  none  "]);
        let out = ex
            .answer(&fallback_analyze("q"), "q", &ctx(&["irrelevant"]), None)
            .await
            .unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn composes_answer_from_extracted_span() {
        // First call = extraction, second call = compose.
        let ex = extractor(vec!["VAT is 7 percent.", "The VAT rate is 7%."]);
        let out = ex
            .answer(
                &fallback_analyze("What is the VAT rate?"),
                "What is the VAT rate?",
                &ctx(&["Thailand levies a value added tax. VAT is 7 percent."]),
                None,
            )
            .await
            .unwrap();
        let resp = out.expect("expected a composed answer");
        assert_eq!(resp.content, "The VAT rate is 7%.");
    }
}
