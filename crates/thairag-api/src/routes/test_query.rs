use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::permission::AccessScope;
use thairag_core::types::{ChatMessage, SearchQuery, WorkspaceId};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::chat::build_searchable_scopes;
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
    pub pipeline_stages: Vec<PipelineStage>,
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

#[derive(Serialize)]
pub struct PipelineStage {
    pub stage: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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
    if claims.sub != "anonymous"
        && let Ok(user_id) = claims.sub.parse::<Uuid>()
    {
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

    let available_scopes = build_searchable_scopes(&state, &scope);
    let settings_scope = state.resolve_scope_for_workspace(ws_id);
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();
    let llm_resp = if let Some(ref pipeline) = scoped_pipeline {
        pipeline
            .process(&messages, &scope, &[], &available_scopes, Some(progress_tx))
            .await
            .map_err(ApiError::from)?
    } else {
        drop(progress_tx);
        p.orchestrator
            .process(&messages, &scope)
            .await
            .map_err(ApiError::from)?
    };
    let generation_ms = gen_start.elapsed().as_millis() as u64;

    // Collect pipeline stage events and merge started+done pairs
    let pipeline_stages: Vec<PipelineStage> = {
        let events: Vec<thairag_core::types::PipelineProgress> =
            std::iter::from_fn(|| progress_rx.try_recv().ok()).collect();
        merge_pipeline_stages(&events)
    };
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

    // Provider info — show the actual model used by response generator (per-agent if configured)
    let (llm_kind, llm_model) = if let Some(ref rg) = p.chat_pipeline_config.response_generator_llm
    {
        (format!("{:?}", rg.kind).to_lowercase(), rg.model.clone())
    } else if let Some(ref shared) = p.chat_pipeline_config.llm {
        (
            format!("{:?}", shared.kind).to_lowercase(),
            shared.model.clone(),
        )
    } else {
        (
            format!("{:?}", p.providers_config.llm.kind).to_lowercase(),
            p.providers_config.llm.model.clone(),
        )
    };
    let provider_info = ProviderInfo {
        llm_kind,
        llm_model,
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
        pipeline_stages,
    }))
}

/// POST /api/km/workspaces/{workspace_id}/test-query-stream
///
/// Same as test_query but streams pipeline progress events via SSE.
/// Events:
///   - event: progress  (PipelineProgress JSON — sent in real-time as stages run)
///   - event: result    (TestQueryResponse JSON — sent once at the end)
///   - data: [DONE]     (sentinel)
pub async fn test_query_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    AppJson(req): AppJson<TestQueryRequest>,
) -> Result<
    (
        header::HeaderMap,
        Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>>,
    ),
    ApiError,
> {
    let total_start = Instant::now();

    // Validate
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
    state
        .km_store
        .get_workspace(ws_id)
        .map_err(|_| ApiError(ThaiRagError::NotFound("Workspace not found".into())))?;

    if claims.sub != "anonymous"
        && let Ok(user_id) = claims.sub.parse::<Uuid>()
    {
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

    let scope = AccessScope::new(vec![ws_id]);
    let available_scopes = build_searchable_scopes(&state, &scope);
    let p = state.providers();
    let retrieval_params = feedback::load_retrieval_params(&state);
    let query_text = req.query.clone();

    // Capture what we need for the background task
    let state_clone = state.clone();
    let scope_clone = scope.clone();

    let sse_stream = async_stream::stream! {
        // Step 1: Search (timed, emit progress)
        yield Ok::<_, std::convert::Infallible>(
            Event::default().event("progress").data(
                serde_json::to_string(&thairag_core::types::PipelineProgress {
                    stage: "search".into(),
                    status: thairag_core::types::StageStatus::Started,
                    duration_ms: None,
                    model: Some(p.providers_config.embedding.model.clone()),
                }).unwrap()
            )
        );

        let search_start = Instant::now();
        let search_query = SearchQuery {
            text: query_text.clone(),
            top_k: retrieval_params.top_k,
            workspace_ids: vec![ws_id],
            unrestricted: false,
        };
        let search_result = p.search_engine.search(&search_query).await;
        let search_ms = search_start.elapsed().as_millis() as u64;

        yield Ok(
            Event::default().event("progress").data(
                serde_json::to_string(&thairag_core::types::PipelineProgress {
                    stage: "search".into(),
                    status: thairag_core::types::StageStatus::Done,
                    duration_ms: Some(search_ms),
                    model: None,
                }).unwrap()
            )
        );

        let mut search_results = match search_result {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({"error": e.to_string()});
                yield Ok(Event::default().event("error").data(err.to_string()));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        // Apply document boost/penalty
        let boost_map = feedback::get_document_boost_map(&state_clone);
        if !boost_map.is_empty() {
            for result in &mut search_results {
                let doc_id_str = result.chunk.doc_id.to_string();
                if let Some(&boost) = boost_map.get(&doc_id_str) {
                    result.score *= boost;
                }
            }
            search_results.sort_by(|a, b| {
                b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        if retrieval_params.min_score_threshold > 0.0 {
            search_results.retain(|r| r.score >= retrieval_params.min_score_threshold);
        }

        let chunks: Vec<RetrievedChunk> = search_results
            .iter()
            .map(|r| {
                let doc_title = state_clone.km_store.get_document(r.chunk.doc_id).ok().map(|d| d.title);
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

        // Step 2: Generate RAG answer — stream pipeline progress in real-time
        let golden_examples = feedback::load_golden_examples_for_workspace(
            &state_clone,
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
            content: query_text.clone(),
        });

        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();

        let pipeline_handle = if let Some(ref pipeline) = p.chat_pipeline {
            let pipeline = pipeline.clone();
            let messages = messages.clone();
            let scope = scope_clone.clone();
            let scopes = available_scopes.clone();
            tokio::spawn(async move {
                pipeline.process(&messages, &scope, &[], &scopes, Some(progress_tx)).await
            })
        } else {
            // Fallback: legacy orchestrator — emit basic progress events
            let orchestrator = p.orchestrator.clone();
            let messages = messages.clone();
            let scope = scope_clone.clone();
            tokio::spawn(async move {
                let _ = progress_tx.send(thairag_core::types::PipelineProgress {
                    stage: "response_generator".into(),
                    status: thairag_core::types::StageStatus::Started,
                    duration_ms: None,
                    model: None,
                });
                let t = Instant::now();
                let result = orchestrator.process(&messages, &scope).await;
                let _ = progress_tx.send(thairag_core::types::PipelineProgress {
                    stage: "response_generator".into(),
                    status: if result.is_ok() {
                        thairag_core::types::StageStatus::Done
                    } else {
                        thairag_core::types::StageStatus::Error
                    },
                    duration_ms: Some(t.elapsed().as_millis() as u64),
                    model: None,
                });
                result
            })
        };

        let gen_start = Instant::now();
        let mut pipeline_handle = pipeline_handle;
        let pipeline_result;

        // Stream progress events in real-time
        let mut channel_open = true;
        loop {
            tokio::select! {
                evt = progress_rx.recv(), if channel_open => {
                    match evt {
                        Some(progress) => {
                            let data = serde_json::to_string(&progress).unwrap();
                            yield Ok(Event::default().event("progress").data(data));
                        }
                        None => {
                            channel_open = false; // Stop polling closed channel
                        }
                    }
                }
                result = &mut pipeline_handle => {
                    // Drain remaining events
                    while let Ok(evt) = progress_rx.try_recv() {
                        let data = serde_json::to_string(&evt).unwrap();
                        yield Ok(Event::default().event("progress").data(data));
                    }
                    pipeline_result = match result {
                        Ok(r) => r,
                        Err(e) => Err(ThaiRagError::LlmProvider(format!("Pipeline task panicked: {e}"))),
                    };
                    break;
                }
            }
        }
        let generation_ms = gen_start.elapsed().as_millis() as u64;
        let total_ms = total_start.elapsed().as_millis() as u64;

        match pipeline_result {
            Ok(llm_resp) => {
                state_clone.metrics.record_tokens(llm_resp.usage.prompt_tokens, llm_resp.usage.completion_tokens);
                persist_usage(&state_clone, llm_resp.usage.prompt_tokens, llm_resp.usage.completion_tokens);

                let (llm_kind, llm_model) = if let Some(ref rg) = p.chat_pipeline_config.response_generator_llm {
                    (format!("{:?}", rg.kind).to_lowercase(), rg.model.clone())
                } else if let Some(ref shared) = p.chat_pipeline_config.llm {
                    (format!("{:?}", shared.kind).to_lowercase(), shared.model.clone())
                } else {
                    (format!("{:?}", p.providers_config.llm.kind).to_lowercase(), p.providers_config.llm.model.clone())
                };

                let response = TestQueryResponse {
                    response_id: Uuid::new_v4().to_string(),
                    query: query_text,
                    chunks,
                    answer: llm_resp.content,
                    usage: TestQueryUsage {
                        prompt_tokens: llm_resp.usage.prompt_tokens,
                        completion_tokens: llm_resp.usage.completion_tokens,
                        total_tokens: llm_resp.usage.prompt_tokens + llm_resp.usage.completion_tokens,
                        chunks_retrieved,
                    },
                    timing: TestQueryTiming { search_ms, generation_ms, total_ms },
                    provider_info: ProviderInfo {
                        llm_kind,
                        llm_model,
                        embedding_kind: format!("{:?}", p.providers_config.embedding.kind).to_lowercase(),
                        embedding_model: p.providers_config.embedding.model.clone(),
                    },
                    pipeline_stages: vec![], // Stages were streamed in real-time
                };

                let data = serde_json::to_string(&response).unwrap();
                yield Ok(Event::default().event("result").data(data));
            }
            Err(e) => {
                let err = serde_json::json!({"error": e.to_string()});
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }

        yield Ok(Event::default().data("[DONE]"));
    };

    // X-Accel-Buffering: no tells reverse proxies (nginx, Cloudflare, etc.)
    // to pass SSE events through immediately instead of buffering them.
    let mut headers = header::HeaderMap::new();
    headers.insert("X-Accel-Buffering", "no".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());

    Ok((
        headers,
        Sse::new(sse_stream).keep_alive(KeepAlive::default()),
    ))
}

/// Merge started+done progress events into a single entry per stage.
/// Model info comes from the Started event; duration from the Done event.
fn merge_pipeline_stages(events: &[thairag_core::types::PipelineProgress]) -> Vec<PipelineStage> {
    use std::collections::HashMap;
    use thairag_core::types::StageStatus;

    // Collect model info from Started events
    let models: HashMap<&str, Option<&str>> = events
        .iter()
        .filter(|e| e.status == StageStatus::Started)
        .map(|e| (e.stage.as_str(), e.model.as_deref()))
        .collect();

    let mut stages = Vec::new();
    for evt in events {
        match evt.status {
            StageStatus::Started => {} // Skip: duration comes from the Done event
            StageStatus::Done => {
                stages.push(PipelineStage {
                    stage: evt.stage.clone(),
                    status: "done".into(),
                    duration_ms: evt.duration_ms,
                    model: models
                        .get(evt.stage.as_str())
                        .and_then(|m| m.map(String::from)),
                });
            }
            StageStatus::Skipped => {
                stages.push(PipelineStage {
                    stage: evt.stage.clone(),
                    status: "skipped".into(),
                    duration_ms: None,
                    model: None,
                });
            }
            StageStatus::Error => {
                stages.push(PipelineStage {
                    stage: evt.stage.clone(),
                    status: "error".into(),
                    duration_ms: evt.duration_ms,
                    model: models
                        .get(evt.stage.as_str())
                        .and_then(|m| m.map(String::from)),
                });
            }
        }
    }
    stages
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
