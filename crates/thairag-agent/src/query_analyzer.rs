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
        };
        let user = ChatMessage {
            role: "user".into(),
            content: query.to_string(),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let content = resp.content.trim();
                // Try to extract JSON from response (handle markdown code blocks)
                let json_str = extract_json(content);
                match serde_json::from_str::<LlmAnalysis>(json_str) {
                    Ok(a) => {
                        debug!(language = %a.language, intent = %a.intent, "Query analyzed by LLM");
                        Ok(parse_llm_analysis(a))
                    }
                    Err(e) => {
                        warn!(error = %e, raw = %content, "Failed to parse LLM analysis, using fallback");
                        Ok(fallback_analyze(query))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM analysis failed, using fallback");
                Ok(fallback_analyze(query))
            }
        }
    }
}

fn extract_json(s: &str) -> &str {
    // Handle ```json ... ``` wrapping
    if let Some(start) = s.find('{')
        && let Some(end) = s.rfind('}')
    {
        return &s[start..=end];
    }
    s
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
