use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, QueryIntent};
use tracing::{debug, warn};

/// Detected language of the user query.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryLanguage {
    Thai,
    English,
    Mixed,
}

/// Complexity level of the query.
#[derive(Debug, Clone, PartialEq)]
pub enum Complexity {
    Simple,
    Moderate,
    Complex,
}

/// Result of query analysis.
#[derive(Debug, Clone)]
pub struct QueryAnalysis {
    pub language: QueryLanguage,
    pub intent: QueryIntent,
    pub complexity: Complexity,
    pub topics: Vec<String>,
    pub needs_context: bool,
}

/// JSON schema mirroring [`LlmAnalysis`] — passed to `generate_structured` so
/// schema-capable providers (Ollama `format`) grammar-constrain the output
/// instead of relying on the prompt alone.
fn analysis_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "language": {"type": "string"},
            "intent": {"type": "string", "enum": [
                "greeting", "retrieval", "comparison", "analysis",
                "clarification", "thanks", "meta"
            ]},
            "complexity": {"type": "string", "enum": ["simple", "moderate", "complex"]},
            "topics": {"type": "array", "items": {"type": "string"}},
            "needs_context": {"type": "boolean"}
        },
        "required": ["language", "intent", "complexity", "topics", "needs_context"]
    })
}

/// LLM JSON response format.
#[derive(Deserialize)]
struct LlmAnalysis {
    #[serde(default = "default_en")]
    language: String,
    #[serde(default = "default_retrieval")]
    intent: String,
    #[serde(default = "default_simple")]
    complexity: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default = "default_true")]
    needs_context: bool,
}

fn default_en() -> String {
    "en".into()
}
fn default_retrieval() -> String {
    "retrieval".into()
}
fn default_simple() -> String {
    "simple".into()
}
fn default_true() -> bool {
    true
}

const DEFAULT_TEMPLATE: &str = "You are a query analyzer. Analyze the user's query and output JSON only.\n\
                Output format:\n\
                {\"language\":\"th\"|\"en\"|\"mixed\",\
                \"intent\":\"greeting\"|\"retrieval\"|\"comparison\"|\"analysis\"|\"clarification\"|\"thanks\"|\"meta\",\
                \"complexity\":\"simple\"|\"moderate\"|\"complex\",\
                \"topics\":[\"topic1\",\"topic2\"],\
                \"needs_context\":true|false}\n\n\
                Rules:\n\
                - greeting: hi/hello/สวัสดี etc.\n\
                - thanks: thank you/ขอบคุณ etc.\n\
                - meta: questions about the bot itself\n\
                - clarification: very short or unclear queries\n\
                - comparison: asks to compare things\n\
                - analysis: asks for deep analysis/explanation\n\
                - retrieval: needs document search\n\
                - needs_context=false for greeting/thanks/meta/clarification\n\
                Output ONLY valid JSON.";

pub struct QueryAnalyzer {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl QueryAnalyzer {
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

    pub async fn analyze(&self, query: &str, _history: &[ChatMessage]) -> Result<QueryAnalysis> {
        let system = ChatMessage {
            role: "system".into(),
            content: self
                .prompts
                .render_or_default("chat.query_analyzer", DEFAULT_TEMPLATE, &[]),
            images: vec![],
        };
        let user = ChatMessage {
            role: "user".into(),
            content: query.to_string(),
            images: vec![],
        };

        match self
            .llm
            .generate_structured(&[system, user], Some(self.max_tokens), &analysis_schema())
            .await
        {
            Ok(resp) => {
                let content = resp.content.trim();
                // Try to extract JSON from response (handle markdown code blocks)
                let json_str = thairag_core::extract_json(content);
                match serde_json::from_str::<LlmAnalysis>(json_str) {
                    Ok(a) => {
                        debug!(language = %a.language, intent = %a.intent, "Query analyzed by LLM");
                        Ok(parse_llm_analysis(a))
                    }
                    Err(e) => {
                        warn!(error = %e, raw = %content, "Failed to parse LLM analysis, using fallback");
                        crate::degradation::record_fallback("query_analyzer");
                        Ok(fallback_analyze(query))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM analysis failed, using fallback");
                crate::degradation::record_fallback("query_analyzer");
                Ok(fallback_analyze(query))
            }
        }
    }
}

fn parse_llm_analysis(a: LlmAnalysis) -> QueryAnalysis {
    let language = match a.language.as_str() {
        "th" | "thai" => QueryLanguage::Thai,
        "mixed" => QueryLanguage::Mixed,
        _ => QueryLanguage::English,
    };

    let intent = match a.intent.as_str() {
        "greeting" => QueryIntent::DirectAnswer,
        "thanks" => QueryIntent::DirectAnswer,
        "meta" => QueryIntent::DirectAnswer,
        "clarification" => QueryIntent::Clarification,
        "comparison" | "analysis" | "retrieval" => QueryIntent::Retrieval,
        _ => QueryIntent::Retrieval,
    };

    let complexity = match a.complexity.as_str() {
        "moderate" | "medium" => Complexity::Moderate,
        "complex" | "hard" => Complexity::Complex,
        _ => Complexity::Simple,
    };

    QueryAnalysis {
        language,
        intent,
        complexity,
        topics: a.topics,
        needs_context: a.needs_context,
    }
}

/// Heuristic fallback when LLM is unavailable or disabled.
pub fn fallback_analyze(query: &str) -> QueryAnalysis {
    let trimmed = query.trim();
    let lower = trimmed.to_lowercase();

    // Language detection via Unicode ranges
    let has_thai = trimmed
        .chars()
        .any(|c| ('\u{0E01}'..='\u{0E5B}').contains(&c));
    let has_latin = trimmed.chars().any(|c| c.is_ascii_alphabetic());
    let language = match (has_thai, has_latin) {
        (true, true) => QueryLanguage::Mixed,
        (true, false) => QueryLanguage::Thai,
        _ => QueryLanguage::English,
    };

    // Intent detection (same regex logic as current orchestrator)
    let intent = crate::orchestrator::classify_intent_pub(query);

    let complexity = if trimmed.len() > 100 {
        Complexity::Complex
    } else if trimmed.len() > 40 {
        Complexity::Moderate
    } else {
        Complexity::Simple
    };

    let needs_context = intent == QueryIntent::Retrieval;

    // Extract simple topics (just the normalized query as a single topic)
    let topics = if needs_context { vec![lower] } else { vec![] };

    QueryAnalysis {
        language,
        intent,
        complexity,
        topics,
        needs_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::LlmResponse;

    /// An LLM that never returns valid JSON — the schema default-impl path
    /// for providers without grammar enforcement.
    struct GarbageLlm;

    #[async_trait::async_trait]
    impl LlmProvider for GarbageLlm {
        fn model_name(&self) -> &str {
            "garbage"
        }
        async fn generate(
            &self,
            _messages: &[ChatMessage],
            _max_tokens: Option<u32>,
        ) -> Result<thairag_core::types::LlmResponse> {
            Ok(LlmResponse {
                content: "certainly! here is some prose, no JSON".into(),
                usage: Default::default(),
            })
        }
    }

    #[tokio::test]
    async fn parse_failure_falls_back_and_records_degradation() {
        let analyzer = QueryAnalyzer::new(Arc::new(GarbageLlm), 256);
        let before = crate::degradation::fallback_counts()
            .iter()
            .find(|(a, _)| *a == "query_analyzer")
            .map(|(_, n)| *n)
            .unwrap_or(0);
        let a = analyzer
            .analyze("what is the withholding rate?", &[])
            .await
            .unwrap();
        // Heuristic fallback still classifies a question as retrieval.
        assert_eq!(a.intent, QueryIntent::Retrieval);
        let after = crate::degradation::fallback_counts()
            .iter()
            .find(|(a, _)| *a == "query_analyzer")
            .map(|(_, n)| *n)
            .unwrap_or(0);
        assert!(
            after > before,
            "fallback must be recorded ({before} -> {after})"
        );
    }
}
