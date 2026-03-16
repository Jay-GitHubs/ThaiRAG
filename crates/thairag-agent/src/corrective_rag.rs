use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::ChatMessage;
use tracing::{debug, info, warn};

use crate::context_curator::CuratedContext;

/// Context quality assessment from CRAG.
#[derive(Debug)]
pub enum ContextAction {
    /// Context is sufficient and relevant — proceed normally.
    Correct,
    /// Context is partially relevant — supplement with web search.
    Ambiguous,
    /// Context is irrelevant or wrong — replace with web search.
    Incorrect,
}

/// Web search result (simplified).
#[derive(Debug, Clone)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Corrective RAG (CRAG) agent: evaluates retrieved context quality and
/// falls back to web search when local knowledge base content is insufficient.
///
/// Flow: Retrieve → Evaluate → { Correct: proceed, Ambiguous: supplement, Incorrect: replace }
pub struct CorrectiveRag {
    llm: Arc<dyn LlmProvider>,
    relevance_threshold: f32,
    web_search_url: Option<String>,
    max_web_results: u32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl CorrectiveRag {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        relevance_threshold: f32,
        web_search_url: Option<String>,
        max_web_results: u32,
        max_tokens: u32,
    ) -> Self {
        Self {
            llm,
            relevance_threshold,
            web_search_url,
            max_web_results,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        relevance_threshold: f32,
        web_search_url: Option<String>,
        max_web_results: u32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            relevance_threshold,
            web_search_url,
            max_web_results,
            max_tokens,
            prompts,
        }
    }

    /// Evaluate whether the retrieved context is sufficient for answering the query.
    pub async fn evaluate_context(
        &self,
        query: &str,
        context: &CuratedContext,
    ) -> Result<ContextAction> {
        if context.chunks.is_empty() {
            return Ok(ContextAction::Incorrect);
        }

        let avg_score = context
            .chunks
            .iter()
            .map(|c| c.relevance_score)
            .sum::<f32>()
            / context.chunks.len() as f32;

        // Fast path: clearly good or clearly bad
        if avg_score >= self.relevance_threshold + 0.2 {
            return Ok(ContextAction::Correct);
        }
        if avg_score < self.relevance_threshold * 0.5 {
            return Ok(ContextAction::Incorrect);
        }

        // LLM-based assessment for ambiguous cases
        let context_preview: String = context
            .chunks
            .iter()
            .take(3)
            .map(|c| truncate(&c.content, 200))
            .collect::<Vec<_>>()
            .join("\n---\n");

        const DEFAULT_CORRECTIVE_RAG_PROMPT: &str = r#"You evaluate whether retrieved context can answer a query.
Return JSON: {"action": "correct"|"ambiguous"|"incorrect", "reason": "brief explanation"}

- "correct": Context directly and sufficiently answers the query
- "ambiguous": Context is partially relevant but may need supplementation
- "incorrect": Context is irrelevant or misleading for this query"#;

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.corrective_rag",
                DEFAULT_CORRECTIVE_RAG_PROMPT,
                &[],
            ),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!(
                "Query: {query}\n\nRetrieved context (avg relevance: {avg_score:.2}):\n{context_preview}"
            ),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = extract_json(resp.content.trim());
                match serde_json::from_str::<CragOutput>(json_str) {
                    Ok(output) => {
                        debug!(
                            action = %output.action,
                            reason = %output.reason,
                            avg_score,
                            "CRAG: context evaluation"
                        );
                        match output.action.as_str() {
                            "correct" => Ok(ContextAction::Correct),
                            "ambiguous" => Ok(ContextAction::Ambiguous),
                            _ => Ok(ContextAction::Incorrect),
                        }
                    }
                    Err(_) => {
                        // Fall back to score-based assessment
                        if avg_score >= self.relevance_threshold {
                            Ok(ContextAction::Correct)
                        } else {
                            Ok(ContextAction::Ambiguous)
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "CRAG: LLM evaluation failed, using score-based fallback");
                if avg_score >= self.relevance_threshold {
                    Ok(ContextAction::Correct)
                } else {
                    Ok(ContextAction::Ambiguous)
                }
            }
        }
    }

    /// Perform a web search as fallback. Uses a configurable search API endpoint.
    /// If no web search URL is configured, returns an empty vec.
    pub async fn web_search(&self, query: &str) -> Result<Vec<WebSearchResult>> {
        let url = match &self.web_search_url {
            Some(u) if !u.is_empty() => u,
            _ => {
                debug!("CRAG: no web search URL configured, skipping web fallback");
                return Ok(Vec::new());
            }
        };

        // Call the web search API (expects a simple JSON API)
        let client = reqwest::Client::new();
        let resp = client
            .get(url)
            .query(&[("q", query), ("n", &self.max_web_results.to_string())])
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => match r.json::<WebSearchApiResponse>().await {
                Ok(api_resp) => {
                    info!(
                        results = api_resp.results.len(),
                        "CRAG: web search completed"
                    );
                    Ok(api_resp
                        .results
                        .into_iter()
                        .map(|r| WebSearchResult {
                            title: r.title,
                            url: r.url,
                            snippet: r.snippet,
                        })
                        .collect())
                }
                Err(e) => {
                    warn!(error = %e, "CRAG: failed to parse web search response");
                    Ok(Vec::new())
                }
            },
            Ok(r) => {
                warn!(status = %r.status(), "CRAG: web search returned error");
                Ok(Vec::new())
            }
            Err(e) => {
                warn!(error = %e, "CRAG: web search request failed");
                Ok(Vec::new())
            }
        }
    }

    /// Distill web search results into clean context using LLM.
    pub async fn distill_web_results(
        &self,
        query: &str,
        web_results: &[WebSearchResult],
    ) -> Result<String> {
        if web_results.is_empty() {
            return Ok(String::new());
        }

        let snippets: String = web_results
            .iter()
            .map(|r| format!("- {} ({}): {}", r.title, r.url, r.snippet))
            .collect::<Vec<_>>()
            .join("\n");

        let system = ChatMessage {
            role: "system".into(),
            content: "Extract and synthesize the relevant information from web search results to answer the query. Return only the distilled factual content, no commentary.".into(),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}\n\nWeb results:\n{snippets}"),
        };

        match self.llm.generate(&[system, user], Some(512)).await {
            Ok(resp) => Ok(resp.content),
            Err(e) => {
                warn!(error = %e, "CRAG: web result distillation failed");
                Ok(snippets)
            }
        }
    }

    /// Check if web search is available.
    pub fn has_web_search(&self) -> bool {
        self.web_search_url.as_ref().is_some_and(|u| !u.is_empty())
    }
}

#[derive(Deserialize)]
struct CragOutput {
    action: String,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct WebSearchApiResponse {
    #[serde(default)]
    results: Vec<WebSearchApiResult>,
}

#[derive(Deserialize)]
struct WebSearchApiResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    snippet: String,
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{')
        && let Some(end) = s.rfind('}')
    {
        return &s[start..=end];
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}
