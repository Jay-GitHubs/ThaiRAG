use std::sync::Arc;

use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

/// Decision from Self-RAG about whether retrieval is needed.
#[derive(Debug)]
pub enum RetrievalDecision {
    /// Retrieval is needed for this query.
    Retrieve,
    /// The model is confident it can answer without retrieval.
    NoRetrieve { confidence: f32 },
}

/// Self-RAG agent: decides whether retrieval from the knowledge base is needed
/// before committing to the full search pipeline. Saves latency and cost when
/// the query is a greeting, general knowledge question, or follow-up that can
/// be answered from conversation context alone.
const DEFAULT_TEMPLATE: &str = r#"You are a retrieval necessity classifier. Given a user query, decide whether \
searching a document knowledge base is required to answer it.

Return JSON only:
{{"needs_retrieval": true/false, "confidence": 0.0-1.0, "reason": "brief explanation"}}

Cases that do NOT need retrieval:
- Greetings, small talk, meta-questions about the assistant
- Simple follow-ups answerable from conversation context
- General knowledge questions (math, definitions, common facts)
- Requests for reformatting/summarizing a previous response

Cases that DO need retrieval:
- Domain-specific questions about documents, policies, procedures
- Questions asking about specific facts, data, or content from the knowledge base
- Comparison or analysis requests that require source material
- Any query where accuracy depends on specific stored documents{{history_summary}}"#;

pub struct SelfRag {
    llm: Arc<dyn LlmProvider>,
    confidence_threshold: f32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl SelfRag {
    pub fn new(llm: Arc<dyn LlmProvider>, confidence_threshold: f32, max_tokens: u32) -> Self {
        Self {
            llm,
            confidence_threshold,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        confidence_threshold: f32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            confidence_threshold,
            max_tokens,
            prompts,
        }
    }

    /// Determine whether retrieval is needed for the given query.
    pub async fn should_retrieve(
        &self,
        query: &str,
        messages: &[ChatMessage],
    ) -> Result<RetrievalDecision> {
        let history_summary = if messages.len() > 2 {
            let recent: Vec<String> = messages
                .iter()
                .rev()
                .take(4)
                .rev()
                .map(|m| format!("{}: {}", m.role, truncate(&m.content, 100)))
                .collect();
            format!("\nRecent conversation:\n{}", recent.join("\n"))
        } else {
            String::new()
        };

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.self_rag",
                DEFAULT_TEMPLATE,
                &[("history_summary", &history_summary)],
            ),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}"),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<SelfRagOutput>(json_str) {
                    Ok(output) => {
                        debug!(
                            needs_retrieval = output.needs_retrieval,
                            confidence = output.confidence,
                            reason = %output.reason,
                            "Self-RAG decision"
                        );
                        if !output.needs_retrieval && output.confidence >= self.confidence_threshold
                        {
                            Ok(RetrievalDecision::NoRetrieve {
                                confidence: output.confidence,
                            })
                        } else {
                            Ok(RetrievalDecision::Retrieve)
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Self-RAG parse failed, defaulting to retrieve");
                        Ok(RetrievalDecision::Retrieve)
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Self-RAG LLM call failed, defaulting to retrieve");
                Ok(RetrievalDecision::Retrieve)
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct SelfRagOutput {
    needs_retrieval: bool,
    confidence: f32,
    #[serde(default)]
    reason: String,
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{')
        && let Some(end) = s.rfind('}')
    {
        return &s[start..=end];
    }
    s
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

/// Heuristic fallback for Self-RAG when no LLM is available.
pub fn heuristic_needs_retrieval(query: &str) -> bool {
    let q = query.trim().to_lowercase();
    let greeting_patterns = [
        "hello",
        "hi",
        "hey",
        "สวัสดี",
        "thanks",
        "thank you",
        "ขอบคุณ",
        "good morning",
        "good afternoon",
        "good evening",
        "bye",
        "goodbye",
    ];
    if greeting_patterns
        .iter()
        .any(|p| q.starts_with(p) || q == *p)
    {
        return false;
    }
    let meta_patterns = [
        "who are you",
        "what can you do",
        "help",
        "คุณเป็นใคร",
        "ทำอะไรได้",
    ];
    if meta_patterns.iter().any(|p| q.contains(p)) {
        return false;
    }
    true
}
