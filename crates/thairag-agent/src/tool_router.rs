use std::sync::Arc;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::LlmProvider;
use thairag_core::types::{ChatMessage, SearchQuery, SearchResult, WorkspaceId};
use thairag_search::HybridSearchEngine;
use tracing::{debug, info, warn};

/// A searchable scope available to the tool router.
#[derive(Debug, Clone)]
pub struct SearchableScope {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub description: Option<String>,
}

/// A tool call decided by the LLM.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    5
}

/// Agent: Tool Router.
/// Lets the LLM decide which knowledge bases to search and with what strategy.
pub struct ToolRouter {
    llm: Arc<dyn LlmProvider>,
    search_engine: Arc<HybridSearchEngine>,
    max_calls: u32,
    max_tokens: u32,
    prompts: Arc<PromptRegistry>,
}

impl ToolRouter {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        search_engine: Arc<HybridSearchEngine>,
        max_calls: u32,
        max_tokens: u32,
    ) -> Self {
        Self {
            llm,
            search_engine,
            max_calls,
            max_tokens,
            prompts: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn new_with_prompts(
        llm: Arc<dyn LlmProvider>,
        search_engine: Arc<HybridSearchEngine>,
        max_calls: u32,
        max_tokens: u32,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            search_engine,
            max_calls,
            max_tokens,
            prompts,
        }
    }

    /// Plan and execute tool calls to gather search results.
    pub async fn plan_and_execute(
        &self,
        query: &str,
        available_scopes: &[SearchableScope],
        unrestricted: bool,
    ) -> Result<Vec<SearchResult>> {
        let calls = self.plan(query, available_scopes).await?;
        info!(calls = calls.len(), "Tool router: planned");

        let mut all_results = Vec::new();
        let allowed_ws: std::collections::HashSet<String> = available_scopes
            .iter()
            .map(|s| s.workspace_id.to_string())
            .collect();

        for call in calls.iter().take(self.max_calls as usize) {
            let results = self
                .execute_call(call, query, &allowed_ws, unrestricted)
                .await;
            match results {
                Ok(mut r) => {
                    debug!(tool = %call.tool, results = r.len(), "Tool call executed");
                    all_results.append(&mut r);
                }
                Err(e) => {
                    warn!(tool = %call.tool, error = %e, "Tool call failed, skipping");
                }
            }
        }

        // Deduplicate
        deduplicate(&mut all_results);
        Ok(all_results)
    }

    async fn plan(
        &self,
        query: &str,
        available_scopes: &[SearchableScope],
    ) -> Result<Vec<ToolCall>> {
        let scopes_desc = if available_scopes.is_empty() {
            "No specific workspaces — use broad_search.".to_string()
        } else {
            available_scopes
                .iter()
                .map(|s| {
                    let desc = s.description.as_deref().unwrap_or("");
                    format!(
                        "  - workspace_id: \"{}\", name: \"{}\" {}",
                        s.workspace_id, s.name, desc
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        const DEFAULT_TOOL_ROUTER: &str = "You are a search strategy planner. Given a query and available knowledge bases, \
decide which tools to call.\n\n\
Available tools:\n\
- search_workspace: Search a specific workspace. Params: workspace_id, query (optional override), top_k\n\
- broad_search: Search across all accessible workspaces. Params: query (optional override), top_k\n\
- keyword_search: Emphasize keyword/BM25 matching. Params: query, top_k\n\
- semantic_search: Emphasize vector/semantic matching. Params: query, top_k\n\n\
Available workspaces:\n{{scopes_desc}}\n\n\
Output JSON array only:\n\
[{\"tool\":\"broad_search\",\"top_k\":5},\
{\"tool\":\"search_workspace\",\"workspace_id\":\"...\",\"top_k\":3}]\n\n\
Rules:\n\
- Use 1-3 tool calls (fewer is better)\n\
- If the query mentions a specific domain, target that workspace\n\
- For broad questions, use broad_search\n\
- For factual lookups, prefer keyword_search\n\
- For conceptual questions, prefer semantic_search\n\
Output ONLY valid JSON array.";

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.tool_router",
                DEFAULT_TOOL_ROUTER,
                &[("scopes_desc", &scopes_desc)],
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
                let json_str = extract_json_array(resp.content.trim());
                match serde_json::from_str::<Vec<ToolCall>>(json_str) {
                    Ok(calls) if !calls.is_empty() => {
                        debug!(calls = calls.len(), "Tool router: LLM planned");
                        Ok(calls)
                    }
                    Ok(_) | Err(_) => {
                        warn!("Tool router: LLM plan parse failed, using broad_search fallback");
                        Ok(vec![ToolCall {
                            tool: "broad_search".into(),
                            workspace_id: None,
                            query: None,
                            top_k: 5,
                        }])
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Tool router: LLM plan failed, using broad_search fallback");
                Ok(vec![ToolCall {
                    tool: "broad_search".into(),
                    workspace_id: None,
                    query: None,
                    top_k: 5,
                }])
            }
        }
    }

    async fn execute_call(
        &self,
        call: &ToolCall,
        original_query: &str,
        allowed_ws: &std::collections::HashSet<String>,
        unrestricted: bool,
    ) -> Result<Vec<SearchResult>> {
        let query_text = call.query.as_deref().unwrap_or(original_query);

        match call.tool.as_str() {
            "search_workspace" => {
                if let Some(ref ws_id) = call.workspace_id {
                    // Security: verify the workspace is in the allowed set
                    if !unrestricted && !allowed_ws.contains(ws_id) {
                        warn!(workspace_id = %ws_id, "Tool router: workspace not in allowed set, skipping");
                        return Ok(vec![]);
                    }
                    let ws_uuid = ws_id.parse::<uuid::Uuid>().map_err(|e| {
                        thairag_core::ThaiRagError::Internal(format!("Invalid workspace_id: {e}"))
                    })?;
                    let sq = SearchQuery {
                        text: query_text.to_string(),
                        top_k: call.top_k,
                        workspace_ids: vec![WorkspaceId(ws_uuid)],
                        unrestricted: false,
                    };
                    self.search_engine.search(&sq).await
                } else {
                    Ok(vec![])
                }
            }
            "broad_search" | "keyword_search" | "semantic_search" => {
                let ws_ids: Vec<WorkspaceId> = allowed_ws
                    .iter()
                    .filter_map(|id| id.parse::<uuid::Uuid>().ok().map(WorkspaceId))
                    .collect();
                let sq = SearchQuery {
                    text: query_text.to_string(),
                    top_k: call.top_k,
                    workspace_ids: ws_ids,
                    unrestricted,
                };
                self.search_engine.search(&sq).await
            }
            other => {
                warn!(tool = %other, "Tool router: unknown tool, skipping");
                Ok(vec![])
            }
        }
    }
}

fn deduplicate(results: &mut Vec<SearchResult>) {
    use std::collections::HashMap;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut keep: Vec<SearchResult> = Vec::new();

    for r in results.iter() {
        let key = r.chunk.chunk_id.to_string();
        if let Some(&idx) = seen.get(&key) {
            if r.score > keep[idx].score {
                keep[idx] = r.clone();
            }
        } else {
            seen.insert(key, keep.len());
            keep.push(r.clone());
        }
    }
    keep.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    *results = keep;
}

fn extract_json_array(s: &str) -> &str {
    if let Some(start) = s.find('[')
        && let Some(end) = s.rfind(']')
    {
        return &s[start..=end];
    }
    s
}
