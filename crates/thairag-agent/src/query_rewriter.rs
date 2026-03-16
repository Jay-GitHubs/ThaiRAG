use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, warn};

use crate::query_analyzer::{Complexity, QueryAnalysis, QueryLanguage};

/// Result of query rewriting.
#[derive(Debug, Clone)]
pub struct RewrittenQueries {
    /// Main rewritten search query.
    pub primary: String,
    /// Decomposed sub-queries for complex questions.
    pub sub_queries: Vec<String>,
    /// Thai/English term expansions.
    pub expanded_terms: Vec<String>,
    /// Hypothetical document snippet for HyDE retrieval.
    pub hyde_query: Option<String>,
}

#[derive(Deserialize)]
struct LlmRewrite {
    #[serde(default)]
    primary: String,
    #[serde(default)]
    sub_queries: Vec<String>,
    #[serde(default)]
    expanded_terms: Vec<String>,
    #[serde(default)]
    hyde_query: Option<String>,
}

const DEFAULT_TEMPLATE: &str = "You are a search query optimizer. Rewrite the user's query for maximum retrieval recall.\n\
                {{complexity_hint}}\n\
                {{language_hint}}\n\n\
                Output JSON only:\n\
                {\"primary\":\"concise keyword-rich search query\",\
                \"sub_queries\":[\"sub-query1\",\"sub-query2\"],\
                \"expanded_terms\":[\"term1_thai\",\"term1_english\"],\
                \"hyde_query\":\"A hypothetical paragraph that would answer this query\"}\n\n\
                Rules:\n\
                - primary: Remove fillers, keep keywords\n\
                - sub_queries: Only for complex queries, break into independent searchable parts\n\
                - expanded_terms: Cross-language keyword pairs (Thai↔English)\n\
                - hyde_query: A short paragraph a document might contain that answers this query\n\
                Output ONLY valid JSON.";

const DEFAULT_FEEDBACK_TEMPLATE: &str = "You are a search query optimizer. The previous search returned low-relevance results.\n\
                Feedback: {{feedback}}\n\n\
                Generate ALTERNATIVE search queries using different keywords, synonyms, \
                or angles. Try broader or more specific terms.\n\n\
                Output JSON only:\n\
                {\"primary\":\"alternative keyword-rich search query\",\
                \"sub_queries\":[\"alt-query1\",\"alt-query2\"],\
                \"expanded_terms\":[\"synonym1\",\"synonym2\"],\
                \"hyde_query\":\"A hypothetical paragraph answering this query differently\"}\n\
                Output ONLY valid JSON.";

pub struct QueryRewriter {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl QueryRewriter {
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

    pub async fn rewrite(&self, query: &str, analysis: &QueryAnalysis) -> Result<RewrittenQueries> {
        let complexity_hint = match analysis.complexity {
            Complexity::Complex => "This is a COMPLEX query. Decompose into 2-4 sub-queries.",
            Complexity::Moderate => {
                "This is a MODERATE query. Generate 1-2 sub-queries if helpful."
            }
            Complexity::Simple => "This is a SIMPLE query. Keep the primary query concise.",
        };

        let language_hint = match analysis.language {
            QueryLanguage::Thai => {
                "The query is in Thai. Generate expanded_terms with English equivalents."
            }
            QueryLanguage::English => {
                "The query is in English. Generate expanded_terms with Thai equivalents if relevant."
            }
            QueryLanguage::Mixed => {
                "The query is mixed Thai/English. Generate expanded_terms in both languages."
            }
        };

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.query_rewriter",
                DEFAULT_TEMPLATE,
                &[
                    ("complexity_hint", complexity_hint),
                    ("language_hint", language_hint),
                ],
            ),
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
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<LlmRewrite>(json_str) {
                    Ok(r) => {
                        debug!(primary = %r.primary, sub_queries = r.sub_queries.len(), "Query rewritten by LLM");
                        Ok(RewrittenQueries {
                            primary: if r.primary.is_empty() {
                                query.to_string()
                            } else {
                                r.primary
                            },
                            sub_queries: r.sub_queries,
                            expanded_terms: r.expanded_terms,
                            hyde_query: r.hyde_query,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse LLM rewrite, using fallback");
                        Ok(fallback_rewrite(query))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM rewrite failed, using fallback");
                Ok(fallback_rewrite(query))
            }
        }
    }

    /// Rewrite with feedback from a failed retrieval attempt.
    pub async fn rewrite_with_feedback(
        &self,
        query: &str,
        analysis: &QueryAnalysis,
        feedback: &str,
    ) -> Result<RewrittenQueries> {
        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.query_rewriter_feedback",
                DEFAULT_FEEDBACK_TEMPLATE,
                &[("feedback", feedback)],
            ),
        };
        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Original query: {query}\nLanguage: {:?}\nComplexity: {:?}",
                analysis.language, analysis.complexity
            ),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<LlmRewrite>(json_str) {
                    Ok(r) => {
                        debug!(primary = %r.primary, "Query rewritten with feedback");
                        Ok(RewrittenQueries {
                            primary: if r.primary.is_empty() {
                                query.to_string()
                            } else {
                                r.primary
                            },
                            sub_queries: r.sub_queries,
                            expanded_terms: r.expanded_terms,
                            hyde_query: r.hyde_query,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse feedback rewrite, using fallback");
                        Ok(fallback_rewrite(query))
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Feedback rewrite failed, using fallback");
                Ok(fallback_rewrite(query))
            }
        }
    }
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') && let Some(end) = s.rfind('}') {
        return &s[start..=end];
    }
    s
}

/// Heuristic fallback: use existing orchestrator normalize logic.
pub fn fallback_rewrite(query: &str) -> RewrittenQueries {
    let normalized = crate::orchestrator::heuristic_normalize_pub(query);
    RewrittenQueries {
        primary: normalized,
        sub_queries: vec![],
        expanded_terms: vec![],
        hyde_query: None,
    }
}
