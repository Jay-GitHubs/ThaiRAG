//! V2 Chat completions endpoint.
//!
//! `POST /v2/chat/completions`
//!
//! Accepts the same request format as V1, but returns an enhanced response
//! with structured metadata including sources, intent, and processing time.
//! V1 remains fully backward compatible.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use serde::Serialize;
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_agent::conversation_memory::MemoryEntry;
use thairag_agent::tool_router::SearchableScope;
use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatChoice, ChatChunkChoice, ChatChunkDelta, ChatCompletionChunk, ChatCompletionRequest,
    ChatMessage, ChatUsage, LlmStreamResponse, MetadataCell, PersonalMemory, PipelineMetadata,
    SessionId, UserId,
};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::chat::{
    build_searchable_scopes, inject_personal_memory_context, load_memories, maybe_auto_summarize,
    maybe_compact_context, persist_usage, retrieve_personal_memories,
};
use crate::routes::feedback;
use crate::store::InferenceLogEntry;

// ── V2 Response Types ────────────────────────────────────────────────

/// A source document referenced in the RAG response.
#[derive(Debug, Clone, Serialize)]
pub struct V2Source {
    pub doc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub score: f32,
}

/// Structured metadata included in V2 responses.
#[derive(Debug, Clone, Serialize)]
pub struct V2Metadata {
    pub search_results_count: u32,
    pub sources: Vec<V2Source>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub processing_time_ms: u64,
}

/// V2 chat completion response — superset of OpenAI format with metadata.
#[derive(Debug, Clone, Serialize)]
pub struct V2ChatCompletionResponse {
    pub id: String,
    pub version: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: ChatUsage,
    pub metadata: V2Metadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// POST /v2/chat/completions
///
/// Enhanced chat completions with structured metadata.
/// Accepts the same request body as V1.
pub async fn v2_chat_completions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    headers: axum::http::HeaderMap,
    AppJson(req): AppJson<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    // ── Request validation (same as V1) ────────────────────────────
    if req.messages.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "messages must not be empty".into(),
        )));
    }
    if req.model != "ThaiRAG-1.0" {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "model not found: {}",
            req.model
        ))));
    }

    let max_messages = state.config.server.max_chat_messages;
    if req.messages.len() > max_messages {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "too many messages: {} (max {})",
            req.messages.len(),
            max_messages
        ))));
    }
    let max_msg_len = state.config.server.max_message_length;
    for (i, msg) in req.messages.iter().enumerate() {
        if msg.content.len() > max_msg_len {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "message[{i}] content too long: {} chars (max {max_msg_len})",
                msg.content.len()
            ))));
        }
    }

    // Per-user concurrent request limiting
    let _request_guard = state
        .user_request_limiter
        .try_acquire(&claims.sub)
        .map_err(|()| {
            ApiError(ThaiRagError::Validation(
                "Too many concurrent requests. Please wait for your previous request to complete."
                    .into(),
            ))
        })?;

    // Per-user rate limiting
    if claims.sub != "anonymous" {
        state
            .user_rate_limiter
            .try_acquire(&claims.sub)
            .map_err(|retry_after| {
                ApiError(ThaiRagError::Validation(format!(
                    "User rate limit exceeded. Retry after {:.0} seconds.",
                    retry_after.ceil()
                )))
            })?;
    }

    // ── Session handling ────────────────────────────────────────────
    let session_id = match &req.session_id {
        Some(id_str) => {
            let uuid = id_str.parse::<Uuid>().map_err(|_| {
                ApiError(ThaiRagError::Validation(format!(
                    "invalid session_id: {id_str}"
                )))
            })?;
            Some(SessionId(uuid))
        }
        None => None,
    };

    let full_messages = if let Some(sid) = session_id {
        let mut msgs = state
            .session_store
            .get_history(&sid)
            .await
            .unwrap_or_default();
        msgs.extend(req.messages.clone());
        msgs
    } else {
        req.messages.clone()
    };

    // ── Scope resolution ────────────────────────────────────────────
    let user_id = resolve_user_id(&state, &claims, &headers);

    let scope = if let Some(uid) = user_id {
        let ws_ids = state.km_store.get_user_workspace_ids(uid);
        if ws_ids.is_empty() {
            AccessScope::none()
        } else {
            AccessScope::new(ws_ids)
        }
    } else if claims.sub == "anonymous" || claims.sub == "api-key" {
        AccessScope::unrestricted()
    } else {
        AccessScope::none()
    };

    let settings_scope = scope
        .workspace_ids
        .first()
        .map(|ws_id| state.resolve_scope_for_workspace(*ws_id))
        .unwrap_or(crate::store::SettingsScope::Global);

    let memories = load_memories(&state, user_id);
    let full_messages = maybe_compact_context(&state, full_messages, session_id, user_id).await;
    let full_messages = maybe_auto_summarize(&state, full_messages, session_id, user_id).await;
    let personal_memories = retrieve_personal_memories(&state, user_id, &full_messages).await;
    let available_scopes = build_searchable_scopes(&state, &scope);

    if req.stream {
        handle_v2_stream(
            state,
            req,
            full_messages,
            scope,
            session_id,
            memories,
            available_scopes,
            user_id,
            personal_memories,
            settings_scope,
        )
        .await
    } else {
        handle_v2_non_stream(
            state,
            req,
            full_messages,
            scope,
            session_id,
            memories,
            available_scopes,
            user_id,
            personal_memories,
            settings_scope,
        )
        .await
    }
}

/// Resolve user ID from claims/headers (same logic as V1).
fn resolve_user_id(
    state: &AppState,
    claims: &AuthClaims,
    headers: &axum::http::HeaderMap,
) -> Option<UserId> {
    if claims.sub == "api-key" {
        headers
            .get("x-openwebui-user-email")
            .and_then(|v| v.to_str().ok())
            .and_then(|email| match state.km_store.get_user_by_email(email) {
                Ok(u) => Some(u.user.id),
                Err(_) => {
                    let name = headers
                        .get("x-openwebui-user-name")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or(email);
                    state
                        .km_store
                        .upsert_user_by_email(
                            email.to_string(),
                            name.to_string(),
                            String::new(),
                            false,
                            "viewer".to_string(),
                        )
                        .ok()
                        .map(|u| u.id)
                }
            })
    } else if claims.sub == "anonymous" {
        None
    } else {
        claims.sub.parse::<Uuid>().ok().map(UserId)
    }
}

/// Resolve LLM provider info.
fn resolve_llm_info(p: &crate::app_state::ProviderBundle) -> (String, String) {
    if let Some(ref rg) = p.chat_pipeline_config.response_generator_llm {
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
    }
}

/// Non-streaming V2 handler — returns full response with metadata.
#[allow(clippy::too_many_arguments)]
async fn handle_v2_non_stream(
    state: AppState,
    req: ChatCompletionRequest,
    full_messages: Vec<ChatMessage>,
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
    personal_memories: Vec<PersonalMemory>,
    settings_scope: crate::store::SettingsScope,
) -> Result<Response, ApiError> {
    let request_start = Instant::now();

    // Inject personal memory context
    let full_messages = inject_personal_memory_context(full_messages, &personal_memories);

    // Inject golden examples
    let golden = feedback::load_golden_examples_for_workspace(&state, None);
    let augmented_messages = if golden.is_empty() {
        full_messages.clone()
    } else {
        let examples_text = golden
            .iter()
            .map(|ex| format!("Q: {}\nA: {}", ex.query, ex.answer))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let mut msgs = vec![ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Here are examples of high-quality answers for reference:\n\n{examples_text}\n\n\
                 Use these examples as a guide for style and quality, but answer based on the retrieved context."
            ),
            images: vec![],
        }];
        msgs.extend(full_messages.clone());
        msgs
    };

    let p = state.providers();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();

    let llm_resp = if let Some(ref pipeline) = scoped_pipeline {
        pipeline
            .process(
                &augmented_messages,
                &scope,
                &memories,
                &available_scopes,
                Some(progress_tx),
                Some(metadata_cell.clone()),
            )
            .await
            .map_err(ApiError::from)?
    } else {
        drop(progress_tx);
        p.orchestrator
            .process(&full_messages, &scope)
            .await
            .map_err(ApiError::from)?
    };

    // Drain pipeline progress events
    let _pipeline_stages: Vec<thairag_core::types::PipelineProgress> =
        std::iter::from_fn(|| progress_rx.try_recv().ok()).collect();

    let processing_time_ms = request_start.elapsed().as_millis() as u64;

    state.metrics.record_tokens(
        llm_resp.usage.prompt_tokens,
        llm_resp.usage.completion_tokens,
    );
    persist_usage(
        &state,
        llm_resp.usage.prompt_tokens,
        llm_resp.usage.completion_tokens,
    );

    // Save to session
    if let Some(sid) = session_id
        && let Some(last_user_msg) = req.messages.last().cloned()
    {
        let assistant_msg = ChatMessage {
            role: "assistant".to_string(),
            content: llm_resp.content.clone(),
            images: vec![],
        };
        state
            .session_store
            .append(sid, last_user_msg, assistant_msg, user_id)
            .await;
    }

    let response_id = format!("chatcmpl-{}", Uuid::new_v4());
    let response_length = llm_resp.content.len() as u32;

    // ── Build V2 metadata from pipeline metadata ────────────────────
    let meta = metadata_cell.lock().unwrap().clone();

    // Build sources from pipeline metadata (chunk info)
    let sources = build_sources_from_metadata(&state, &scope, &meta);

    let v2_metadata = V2Metadata {
        search_results_count: meta.chunks_retrieved.unwrap_or(0),
        sources,
        intent: meta.intent.clone(),
        processing_time_ms,
    };

    // ── Inference Logging ──────────────────────────────────────────
    {
        let user_query = req
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let (llm_kind, llm_model) = resolve_llm_info(&p);
        let entry = InferenceLogEntry {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            user_id: user_id.map(|u| u.0.to_string()),
            workspace_id: scope.workspace_ids.first().map(|w| w.0.to_string()),
            org_id: None,
            dept_id: None,
            session_id: session_id.map(|s| s.0.to_string()),
            response_id: response_id.clone(),
            query_text: user_query.chars().take(2000).collect(),
            detected_language: meta.language,
            intent: meta.intent,
            complexity: meta.complexity,
            llm_kind,
            llm_model,
            settings_scope: format!("{:?}", settings_scope),
            prompt_tokens: llm_resp.usage.prompt_tokens,
            completion_tokens: llm_resp.usage.completion_tokens,
            total_ms: processing_time_ms,
            search_ms: meta.search_ms,
            generation_ms: meta.generation_ms,
            chunks_retrieved: meta.chunks_retrieved,
            avg_chunk_score: meta.avg_chunk_score,
            self_rag_decision: meta.self_rag_decision,
            self_rag_confidence: meta.self_rag_confidence,
            quality_guard_pass: meta.quality_guard_pass,
            relevance_score: meta.relevance_score,
            hallucination_score: meta.hallucination_score,
            completeness_score: meta.completeness_score,
            pipeline_route: meta.pipeline_route,
            agents_used: "[]".into(),
            status: "success".into(),
            error_message: None,
            response_length,
            feedback_score: None,
            input_guardrails_pass: meta.input_guardrails_pass,
            output_guardrails_pass: meta.output_guardrails_pass,
            guardrail_violation_codes: meta
                .guardrail_violations
                .iter()
                .map(|v| v.code.as_str())
                .collect::<Vec<_>>()
                .join(","),
        };
        let store = state.km_store.clone();
        tokio::spawn(async move {
            store.insert_inference_log(&entry);
        });
    }

    let response = V2ChatCompletionResponse {
        id: response_id,
        version: "v2".to_string(),
        object: "chat.completion".to_string(),
        created: Utc::now().timestamp(),
        model: "ThaiRAG-1.0".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: llm_resp.content,
                images: vec![],
            },
            finish_reason: "stop".to_string(),
        }],
        usage: ChatUsage {
            prompt_tokens: llm_resp.usage.prompt_tokens,
            completion_tokens: llm_resp.usage.completion_tokens,
            total_tokens: llm_resp.usage.prompt_tokens + llm_resp.usage.completion_tokens,
        },
        metadata: v2_metadata,
        session_id: session_id.map(|s| s.to_string()),
    };

    Ok(Json(response).into_response())
}

/// Streaming V2 handler — streams chunks with V2 metadata in the final chunk.
#[allow(clippy::too_many_arguments)]
async fn handle_v2_stream(
    state: AppState,
    req: ChatCompletionRequest,
    full_messages: Vec<ChatMessage>,
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
    personal_memories: Vec<PersonalMemory>,
    settings_scope: crate::store::SettingsScope,
) -> Result<Response, ApiError> {
    let request_start = Instant::now();

    // Inject personal memory + golden examples
    let full_messages = inject_personal_memory_context(full_messages, &personal_memories);
    let golden = feedback::load_golden_examples_for_workspace(&state, None);
    let augmented_messages = if golden.is_empty() {
        full_messages.clone()
    } else {
        let examples_text = golden
            .iter()
            .map(|ex| format!("Q: {}\nA: {}", ex.query, ex.answer))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        let mut msgs = vec![ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Here are examples of high-quality answers for reference:\n\n{examples_text}\n\n\
                 Use these examples as a guide for style and quality, but answer based on the retrieved context."
            ),
            images: vec![],
        }];
        msgs.extend(full_messages.clone());
        msgs
    };

    let p = state.providers();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);

    let completion_id = format!("chatcmpl-{}", Uuid::new_v4());
    let completion_id_clone = completion_id.clone();

    let stream_result: LlmStreamResponse = if let Some(ref pipeline) = scoped_pipeline {
        pipeline
            .process_stream(
                &augmented_messages,
                &scope,
                &memories,
                &available_scopes,
                None,
                Some(metadata_cell.clone()),
            )
            .await
            .map_err(ApiError::from)?
    } else {
        p.orchestrator
            .process_stream(&full_messages, &scope)
            .await
            .map_err(ApiError::from)?
    };

    let usage_handle = stream_result.usage.clone();
    let state_clone = state.clone();
    let scope_clone = scope.clone();
    let req_messages = req.messages.clone();

    let sse_stream = async_stream::stream! {
        // First chunk: role
        let first_chunk = ChatCompletionChunk {
            id: completion_id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created: Utc::now().timestamp(),
            model: "ThaiRAG-1.0".to_string(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatChunkDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&first_chunk).unwrap();
        yield Ok::<_, std::convert::Infallible>(Event::default().data(json));

        // Content chunks
        let mut content_stream = stream_result.stream;
        let mut full_content = String::new();
        while let Some(chunk_result) = content_stream.next().await {
            match chunk_result {
                Ok(text) => {
                    full_content.push_str(&text);
                    let chunk = ChatCompletionChunk {
                        id: completion_id_clone.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created: Utc::now().timestamp(),
                        model: "ThaiRAG-1.0".to_string(),
                        choices: vec![ChatChunkChoice {
                            index: 0,
                            delta: ChatChunkDelta {
                                role: None,
                                content: Some(text),
                            },
                            finish_reason: None,
                        }],
                        usage: None,
                    };
                    let json = serde_json::to_string(&chunk).unwrap();
                    yield Ok(Event::default().data(json));
                }
                Err(_) => break,
            }
        }

        // Finish chunk with usage + V2 metadata
        let usage = usage_handle.lock().unwrap().clone().unwrap_or_default();
        let processing_time_ms = request_start.elapsed().as_millis() as u64;
        let meta = metadata_cell.lock().unwrap().clone();

        let sources = build_sources_from_metadata(&state_clone, &scope_clone, &meta);

        let v2_meta = V2Metadata {
            search_results_count: meta.chunks_retrieved.unwrap_or(0),
            sources,
            intent: meta.intent.clone(),
            processing_time_ms,
        };

        // Emit V2 metadata as a separate event before [DONE]
        let meta_json = serde_json::to_string(&v2_meta).unwrap();
        yield Ok(Event::default().event("metadata").data(meta_json));

        // Usage chunk (OpenAI-compatible)
        let usage_chunk = ChatCompletionChunk {
            id: completion_id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created: Utc::now().timestamp(),
            model: "ThaiRAG-1.0".to_string(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatChunkDelta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(ChatUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.prompt_tokens + usage.completion_tokens,
            }),
        };
        let json = serde_json::to_string(&usage_chunk).unwrap();
        yield Ok(Event::default().data(json));

        // Record metrics
        state_clone.metrics.record_tokens(usage.prompt_tokens, usage.completion_tokens);
        persist_usage(&state_clone, usage.prompt_tokens, usage.completion_tokens);

        // Save to session
        if let Some(sid) = session_id
            && let Some(last_user_msg) = req_messages.last().cloned()
        {
            let assistant_msg = ChatMessage {
                role: "assistant".to_string(),
                content: full_content,
                images: vec![],
            };
            state_clone.session_store.append(sid, last_user_msg, assistant_msg, user_id).await;
        }

        yield Ok(Event::default().data("[DONE]"));
    };

    Ok(Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// Build V2 source references from pipeline metadata.
///
/// Uses the search results captured in pipeline metadata to construct
/// source references with document IDs, titles, and relevance scores.
fn build_sources_from_metadata(
    state: &AppState,
    scope: &AccessScope,
    meta: &PipelineMetadata,
) -> Vec<V2Source> {
    // The pipeline metadata doesn't store individual chunk info directly,
    // but we can do a quick search to get the source documents if chunks were retrieved.
    // For now, we report workspace-level sources based on available scope.
    // In a production system, the pipeline metadata would carry per-chunk info.

    // If no chunks were retrieved, return empty
    if meta.chunks_retrieved.unwrap_or(0) == 0 {
        return vec![];
    }

    // Return workspace IDs as high-level sources (the pipeline metadata
    // doesn't expose individual chunk references at this level).
    // The v2/search endpoint provides detailed per-document results.
    scope
        .workspace_ids
        .iter()
        .filter_map(|ws_id| {
            state
                .km_store
                .get_workspace(*ws_id)
                .ok()
                .map(|ws| V2Source {
                    doc_id: ws_id.to_string(),
                    title: Some(ws.name),
                    score: meta.avg_chunk_score.unwrap_or(0.0),
                })
        })
        .collect()
}
