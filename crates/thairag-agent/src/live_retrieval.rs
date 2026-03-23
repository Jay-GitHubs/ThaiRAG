use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use thairag_core::PromptRegistry;
use thairag_core::error::Result;
use thairag_core::traits::{LlmProvider, McpClient};
use thairag_core::types::{ChatMessage, ChunkId, ConnectorStatus, DocId, McpConnectorConfig};
use tracing::{debug, info, warn};

use crate::context_curator::{CuratedChunk, CuratedContext};

/// Live Source Retrieval agent: fetches content from MCP connectors in real time
/// when the vector DB has no relevant results.
pub struct LiveRetrieval {
    llm: Arc<dyn LlmProvider>,
    max_tokens: u32,
    timeout: Duration,
    max_connectors: u32,
    max_content_chars: usize,
    connect_timeout: Duration,
    read_timeout: Duration,
    prompts: Arc<PromptRegistry>,
}

impl LiveRetrieval {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        max_tokens: u32,
        timeout: Duration,
        max_connectors: u32,
        max_content_chars: usize,
        connect_timeout: Duration,
        read_timeout: Duration,
        prompts: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            llm,
            max_tokens,
            timeout,
            max_connectors,
            max_content_chars,
            connect_timeout,
            read_timeout,
            prompts,
        }
    }

    /// Fetch live context from MCP connectors.
    /// Connects to active connectors, reads resources, and builds a CuratedContext.
    pub async fn fetch_live_context(
        &self,
        query: &str,
        connectors: &[McpConnectorConfig],
    ) -> Result<CuratedContext> {
        let active: Vec<&McpConnectorConfig> = connectors
            .iter()
            .filter(|c| c.status == ConnectorStatus::Active)
            .collect();

        if active.is_empty() {
            debug!("LiveRetrieval: no active connectors");
            return Ok(CuratedContext::default());
        }

        // Select connectors (heuristic keyword match, LLM fallback if too many)
        let selected = if active.len() as u32 <= self.max_connectors {
            active
        } else {
            self.select_connectors(query, &active).await?
        };

        info!(count = selected.len(), "LiveRetrieval: querying connectors");

        // Per-connector content budget
        let chars_per_connector = self.max_content_chars / selected.len().max(1);

        // Connect and fetch from each connector in parallel (with overall timeout)
        let handles: Vec<_> = selected
            .into_iter()
            .map(|cfg| {
                let cfg = cfg.clone();
                let connect_timeout = self.connect_timeout;
                let read_timeout = self.read_timeout;
                let max_chars = chars_per_connector;
                tokio::spawn(async move {
                    fetch_from_connector(cfg, connect_timeout, read_timeout, max_chars).await
                })
            })
            .collect();

        let mut all_results: Vec<Vec<FetchedContent>> = Vec::new();
        let deadline = tokio::time::Instant::now() + self.timeout;
        for handle in handles {
            match tokio::time::timeout_at(deadline, handle).await {
                Ok(Ok(fetched)) => all_results.push(fetched),
                Ok(Err(e)) => warn!(error = %e, "LiveRetrieval: task panicked"),
                Err(_) => {
                    warn!("LiveRetrieval: overall timeout exceeded");
                    break;
                }
            }
        }

        // Build CuratedContext from all fetched content
        let mut chunks = Vec::new();
        for (idx, fetched) in all_results.into_iter().flatten().enumerate() {
            chunks.push(CuratedChunk {
                index: idx,
                content: fetched.content,
                relevance_score: 0.5, // Neutral — let response generator judge
                source_doc_id: DocId::new(),
                source_chunk_id: ChunkId::new(),
                source_doc_title: Some(fetched.title),
            });
        }

        let total_tokens_est = chunks.iter().map(|c| c.content.len() / 4).sum();
        info!(
            chunks = chunks.len(),
            tokens_est = total_tokens_est,
            "LiveRetrieval: fetched live context"
        );

        Ok(CuratedContext {
            chunks,
            total_tokens_est,
        })
    }

    /// Use LLM to select the most relevant connectors when there are too many.
    async fn select_connectors<'a>(
        &self,
        query: &str,
        connectors: &[&'a McpConnectorConfig],
    ) -> Result<Vec<&'a McpConnectorConfig>> {
        let connector_list: String = connectors
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. {} — {}", i + 1, c.name, c.description))
            .collect::<Vec<_>>()
            .join("\n");

        let default_prompt = r#"Given a user query and a list of data connectors, select the most relevant ones.
Return JSON only: {"selected": [1, 3]} (1-based indices).
Select at most {{max}} connectors."#;

        let system = ChatMessage {
            role: "system".into(),
            content: self.prompts.render_or_default(
                "chat.live_retrieval_select",
                default_prompt,
                &[("max", &self.max_connectors.to_string())],
            ),
        };

        let user = ChatMessage {
            role: "user".into(),
            content: format!("Query: {query}\n\nConnectors:\n{connector_list}"),
        };

        match self
            .llm
            .generate(&[system, user], Some(self.max_tokens))
            .await
        {
            Ok(resp) => {
                let json_str = thairag_core::extract_json(resp.content.trim());
                #[derive(Deserialize)]
                struct Selection {
                    #[serde(default)]
                    selected: Vec<usize>,
                }
                match serde_json::from_str::<Selection>(json_str) {
                    Ok(sel) => {
                        let picked: Vec<&McpConnectorConfig> = sel
                            .selected
                            .iter()
                            .filter_map(|&i| connectors.get(i.wrapping_sub(1)).copied())
                            .take(self.max_connectors as usize)
                            .collect();
                        if picked.is_empty() {
                            // Fallback: take first N
                            Ok(connectors
                                .iter()
                                .take(self.max_connectors as usize)
                                .copied()
                                .collect())
                        } else {
                            Ok(picked)
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "LiveRetrieval: connector selection parse failed");
                        Ok(connectors
                            .iter()
                            .take(self.max_connectors as usize)
                            .copied()
                            .collect())
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LiveRetrieval: connector selection LLM failed");
                Ok(connectors
                    .iter()
                    .take(self.max_connectors as usize)
                    .copied()
                    .collect())
            }
        }
    }
}

/// Content fetched from a single MCP resource.
struct FetchedContent {
    title: String,
    content: String,
}

/// Connect to a single MCP connector, list resources, read them, and return text content.
async fn fetch_from_connector(
    config: McpConnectorConfig,
    connect_timeout: Duration,
    read_timeout: Duration,
    max_chars: usize,
) -> Vec<FetchedContent> {
    let connector_name = config.name.clone();
    let resource_filters = config.resource_filters.clone();

    let mut client = thairag_mcp::client::RmcpClient::new(config, connect_timeout, read_timeout);

    if let Err(e) = client.connect().await {
        warn!(connector = %connector_name, error = %e, "LiveRetrieval: connect failed");
        return vec![];
    }

    let resources = match client.list_resources().await {
        Ok(r) => r,
        Err(e) => {
            warn!(connector = %connector_name, error = %e, "LiveRetrieval: list_resources failed");
            let _ = client.disconnect().await;
            return vec![];
        }
    };

    // Filter resources by configured patterns (if any)
    let filtered: Vec<_> = if resource_filters.is_empty() {
        resources
    } else {
        resources
            .into_iter()
            .filter(|r| {
                resource_filters
                    .iter()
                    .any(|pat| r.uri.contains(pat) || r.name.contains(pat))
            })
            .collect()
    };

    let mut fetched = Vec::new();
    let mut total_chars = 0usize;

    for resource in &filtered {
        if total_chars >= max_chars {
            break;
        }

        match client.read_resource(&resource.uri).await {
            Ok(content) => {
                let text = String::from_utf8_lossy(&content.data);
                // Strip HTML tags for cleaner text
                let clean = strip_html_tags(&text);
                let remaining = max_chars.saturating_sub(total_chars);
                let truncated = thairag_core::safe_truncate(&clean, remaining).to_string();
                total_chars += truncated.len();

                fetched.push(FetchedContent {
                    title: format!("[{}] {}", connector_name, resource.name),
                    content: truncated,
                });
            }
            Err(e) => {
                warn!(
                    connector = %connector_name,
                    resource = %resource.name,
                    error = %e,
                    "LiveRetrieval: read_resource failed"
                );
            }
        }
    }

    let _ = client.disconnect().await;
    debug!(
        connector = %connector_name,
        resources = fetched.len(),
        chars = total_chars,
        "LiveRetrieval: fetched"
    );
    fetched
}

/// Simple HTML tag stripper (no dependency needed for this).
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}
