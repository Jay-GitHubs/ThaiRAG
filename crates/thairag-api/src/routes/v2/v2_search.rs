//! V2 Search endpoint.
//!
//! `POST /v2/search`
//!
//! Dedicated search endpoint that returns raw search results without LLM generation.
//! Only available in V2 — V1 only has chat.

use std::time::Instant;

use axum::extract::State;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::{SearchQuery, UserId, WorkspaceId};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::plugin_hooks;
use crate::routes::feedback;

// ── Request / Response Types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct V2SearchRequest {
    pub query: String,
    /// Workspace IDs to search within. If empty, searches all accessible workspaces.
    #[serde(default)]
    pub workspace_ids: Vec<Uuid>,
    /// Maximum number of results to return (default: 10).
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    10
}

#[derive(Debug, Serialize)]
pub struct V2SearchResponse {
    pub results: Vec<V2SearchResult>,
    pub processing_time_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct V2SearchResult {
    pub doc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    pub score: f32,
    pub metadata: V2SearchResultMetadata,
}

#[derive(Debug, Serialize)]
pub struct V2SearchResultMetadata {
    pub chunk_id: String,
    pub chunk_index: usize,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_numbers: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
}

/// POST /v2/search
///
/// Search across workspaces without LLM generation.
/// Returns raw search results with relevance scores.
pub async fn v2_search(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<V2SearchRequest>,
) -> Result<Json<V2SearchResponse>, ApiError> {
    let start = Instant::now();

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
    if req.top_k == 0 || req.top_k > 100 {
        return Err(ApiError(ThaiRagError::Validation(
            "top_k must be between 1 and 100".into(),
        )));
    }

    // ── Resolve user scope ──────────────────────────────────────────
    let user_id = if claims.sub == "anonymous" || claims.sub == "api-key" {
        None
    } else {
        claims.sub.parse::<Uuid>().ok().map(UserId)
    };

    let user_workspace_ids: Vec<WorkspaceId> = if let Some(uid) = user_id {
        state.km_store.get_user_workspace_ids(uid)
    } else {
        // Anonymous / API key: all workspaces
        vec![]
    };

    // Determine which workspace IDs to search
    let (search_ws_ids, unrestricted) = if req.workspace_ids.is_empty() {
        if user_id.is_some() {
            // Search user's accessible workspaces
            (user_workspace_ids.clone(), false)
        } else {
            // Anonymous: unrestricted
            (vec![], true)
        }
    } else {
        // Caller specified workspace IDs — verify access
        let requested: Vec<WorkspaceId> = req
            .workspace_ids
            .iter()
            .map(|id| WorkspaceId(*id))
            .collect();

        if user_id.is_some() {
            // Filter to only accessible workspaces (unless super admin)
            let is_super = user_id
                .and_then(|uid| state.km_store.get_user(uid).ok())
                .map(|u| u.is_super_admin)
                .unwrap_or(false);

            if is_super {
                (requested, false)
            } else {
                let accessible: Vec<WorkspaceId> = requested
                    .into_iter()
                    .filter(|ws| user_workspace_ids.contains(ws))
                    .collect();
                if accessible.is_empty() {
                    return Err(ApiError(ThaiRagError::Authorization(
                        "No access to the specified workspaces".into(),
                    )));
                }
                (accessible, false)
            }
        } else {
            // Anonymous: allow all requested
            (requested, false)
        }
    };

    // ── Execute search ──────────────────────────────────────────────
    let p = state.providers();
    let retrieval_params = feedback::load_retrieval_params(&state);

    // Apply pre-search plugins to transform the query
    let transformed_query = plugin_hooks::apply_pre_search(&state.plugin_registry, &req.query);

    let search_query = SearchQuery {
        text: transformed_query,
        top_k: req.top_k.min(retrieval_params.top_k.max(req.top_k)),
        workspace_ids: search_ws_ids,
        unrestricted,
    };

    let mut search_results = p
        .search_engine
        .search(&search_query)
        .await
        .map_err(ApiError::from)?;

    // Apply post-search plugins to filter/re-rank results
    search_results = plugin_hooks::apply_post_search(&state.plugin_registry, search_results);

    // Apply document boost/penalty from feedback
    let boost_map = feedback::get_document_boost_map(&state);
    if !boost_map.is_empty() {
        for result in &mut search_results {
            let doc_id_str = result.chunk.doc_id.to_string();
            if let Some(&boost) = boost_map.get(&doc_id_str) {
                result.score *= boost;
            }
        }
        search_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Apply min score threshold
    if retrieval_params.min_score_threshold > 0.0 {
        search_results.retain(|r| r.score >= retrieval_params.min_score_threshold);
    }

    // Truncate to requested top_k
    search_results.truncate(req.top_k);

    // ── Build response ──────────────────────────────────────────────
    let results: Vec<V2SearchResult> = search_results
        .iter()
        .map(|r| {
            let doc_title = state
                .km_store
                .get_document(r.chunk.doc_id)
                .ok()
                .map(|d| d.title);
            let meta = r.chunk.metadata.as_ref();
            V2SearchResult {
                doc_id: r.chunk.doc_id.to_string(),
                title: doc_title,
                content: r.chunk.content.clone(),
                score: r.score,
                metadata: V2SearchResultMetadata {
                    chunk_id: r.chunk.chunk_id.to_string(),
                    chunk_index: r.chunk.chunk_index,
                    workspace_id: r.chunk.workspace_id.to_string(),
                    page_numbers: meta.and_then(|m| m.page_numbers.clone()),
                    section_title: meta.and_then(|m| m.section_title.clone()),
                },
            }
        })
        .collect();

    let processing_time_ms = start.elapsed().as_millis() as u64;

    Ok(Json(V2SearchResponse {
        results,
        processing_time_ms,
    }))
}
