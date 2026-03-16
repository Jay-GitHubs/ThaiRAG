use std::time::Instant;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::permission::AccessScope;
use thairag_core::types::{ChatMessage, SearchQuery, WorkspaceId};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::feedback;

#[derive(Deserialize)]
pub struct TestQueryRequest {
    pub query: String,
}

#[derive(Serialize)]
pub struct TestQueryResponse {
    /// Unique ID for this response (used for feedback).
    pub response_id: String,
    pub query: String,
    pub chunks: Vec<RetrievedChunk>,
    pub answer: String,
    pub usage: TestQueryUsage,
    pub timing: TestQueryTiming,
    pub provider_info: ProviderInfo,
}

#[derive(Serialize)]
pub struct RetrievedChunk {
    pub chunk_id: String,
    pub doc_id: String,
    pub content: String,
    pub score: f32,
    pub chunk_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_numbers: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_title: Option<String>,
}

#[derive(Serialize)]
pub struct TestQueryUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub chunks_retrieved: usize,
}

#[derive(Serialize)]
pub struct TestQueryTiming {
    pub search_ms: u64,
    pub generation_ms: u64,
    pub total_ms: u64,
}

#[derive(Serialize)]
pub struct ProviderInfo {
    pub llm_kind: String,
    pub llm_model: String,
    pub embedding_kind: String,
    pub embedding_model: String,
}

/// POST /api/km/workspaces/{workspace_id}/test-query
///
/// Run a search + RAG answer against a specific workspace.
/// Returns retrieved chunks (with scores), the generated answer,
/// timing breakdown, and provider info.
pub async fn test_query(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    AppJson(req): AppJson<TestQueryRequest>,
) -> Result<Json<TestQueryResponse>, ApiError> {
    let total_start = Instant::now();

    // Validate query
    if req.query.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "query must not be empty".into(),
        )));
    }
    let max_len = state.config.server.max_message_length;
    if req.query.len() > max_len {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "query too long: {} chars (max {max_len})",
            req.query.len()
        ))));
    }

    let ws_id = WorkspaceId(workspace_id);

    // Verify the workspace exists
    state
        .km_store
        .get_workspace(ws_id)
        .map_err(|_| ApiError(ThaiRagError::NotFound("Workspace not found".into())))?;

    // Verify user has access (unless anonymous/unrestricted)
    if claims.sub != "anonymous" && let Ok(user_id) = claims.sub.parse::<Uuid>() {
        let user_ws_ids = state
            .km_store
            .get_user_workspace_ids(thairag_core::types::UserId(user_id));
        if !user_ws_ids.contains(&ws_id) {
            let is_super = state
                .km_store
                .get_user(thairag_core::types::UserId(user_id))
                .map(|u| u.is_super_admin)
                .unwrap_or(false);
            if !is_super {
                return Err(ApiError(ThaiRagError::Authorization(
                    "No access to this workspace".into(),
                )));
            }
        }
    }

    // Build scope for just this workspace
    let scope = AccessScope::new(vec![ws_id]);

    // Step 1: Search for relevant chunks (timed)
    let p = state.providers();
    let retrieval_params = feedback::load_retrieval_params(&state);
    let search_start = Instant::now();
    let search_query = SearchQuery {
        text: req.query.clone(),
        top_k: retrieval_params.top_k,
        workspace_ids: vec![ws_id],
        unrestricted: false,
    };
    let mut search_results = p
        .search_engine
        .search(&search_query)
        .await
        .map_err(ApiError::from)?;
    let search_ms = search_start.elapsed().as_millis() as u64;

    // Apply document boost/penalty from feedback
    let boost_map = feedback::get_document_boost_map(&state);
    if !boost_map.is_empty() {
        for result in &mut search_results {
            let doc_id_str = result.chunk.doc_id.to_string();
            if let Some(&boost) = boost_map.get(&doc_id_str) {
                result.score *= boost;
            }
        }
        // Re-sort by boosted scores
        search_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Apply min score threshold from retrieval params
    if retrieval_params.min_score_threshold > 0.0 {
        search_results.retain(|r| r.score >= retrieval_params.min_score_threshold);
    }

    // Build retrieved chunks response (with doc titles)
    let chunks: Vec<RetrievedChunk> = search_results
        .iter()
        .map(|r| {
            let doc_title = state
                .km_store
                .get_document(r.chunk.doc_id)
                .ok()
                .map(|d| d.title);
            let meta = r.chunk.metadata.as_ref();
            RetrievedChunk {
                chunk_id: r.chunk.chunk_id.to_string(),
                doc_id: r.chunk.doc_id.to_string(),
                content: r.chunk.content.clone(),
                score: r.score,
                chunk_index: r.chunk.chunk_index,
                page_numbers: meta.and_then(|m| m.page_numbers.clone()),
                section_title: meta.and_then(|m| m.section_title.clone()),
                doc_title,
            }
        })
        .collect();

    let chunks_retrieved = chunks.len();

    // Step 2: Generate RAG answer (timed)
    let gen_start = Instant::now();

    // Inject golden examples as few-shot demonstrations
    let golden_examples = feedback::load_golden_examples_for_workspace(
        &state,
        Some(workspace_id.to_string()).as_deref(),
    );
    let mut messages = Vec::new();
    if !golden_examples.is_empty() {
        let examples_text = golden_examples
            .iter()
            .map(|ex| format!("Q: {}\nA: {}", ex.query, ex.answer))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Here are examples of high-quality answers for reference:\n\n{examples_text}\n\n\
                 Use these examples as a guide for style and quality, but answer based on the retrieved context."
            ),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: req.query.clone(),
    });

    let llm_resp = if let Some(ref pipeline) = p.chat_pipeline {
        pipeline
            .process(&messages, &scope, &[], &[])
            .await
            .map_err(ApiError::from)?
    } else {
        p.orchestrator
            .process(&messages, &scope)
            .await
            .map_err(ApiError::from)?
    };
    let generation_ms = gen_start.elapsed().as_millis() as u64;
    let total_ms = total_start.elapsed().as_millis() as u64;

    state.metrics.record_tokens(
        llm_resp.usage.prompt_tokens,
        llm_resp.usage.completion_tokens,
    );

    // Persist usage to KV store for the Usage page
    persist_usage(
        &state,
        llm_resp.usage.prompt_tokens,
        llm_resp.usage.completion_tokens,
    );

    // Provider info for cost estimation on the client
    let provider_info = ProviderInfo {
        llm_kind: format!("{:?}", p.providers_config.llm.kind).to_lowercase(),
        llm_model: p.providers_config.llm.model.clone(),
        embedding_kind: format!("{:?}", p.providers_config.embedding.kind).to_lowercase(),
        embedding_model: p.providers_config.embedding.model.clone(),
    };

    let response_id = Uuid::new_v4().to_string();

    Ok(Json(TestQueryResponse {
        response_id,
        query: req.query,
        chunks,
        answer: llm_resp.content,
        usage: TestQueryUsage {
            prompt_tokens: llm_resp.usage.prompt_tokens,
            completion_tokens: llm_resp.usage.completion_tokens,
            total_tokens: llm_resp.usage.prompt_tokens + llm_resp.usage.completion_tokens,
            chunks_retrieved,
        },
        timing: TestQueryTiming {
            search_ms,
            generation_ms,
            total_ms,
        },
        provider_info,
    }))
}

/// Persist cumulative token usage to KV store so it survives restarts.
fn persist_usage(state: &AppState, prompt: u32, completion: u32) {
    let key = "usage:tokens";
    let (prev_prompt, prev_completion) = state
        .km_store
        .get_setting(key)
        .and_then(|v| serde_json::from_str::<(u64, u64)>(&v).ok())
        .unwrap_or((0, 0));

    let new_prompt = prev_prompt + prompt as u64;
    let new_completion = prev_completion + completion as u64;

    if let Ok(json) = serde_json::to_string(&(new_prompt, new_completion)) {
        state.km_store.set_setting(key, &json);
    }
}
