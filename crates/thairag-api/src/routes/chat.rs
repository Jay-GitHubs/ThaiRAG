use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_agent::conversation_memory::MemoryEntry;
use thairag_agent::tool_router::SearchableScope;
use thairag_auth::AuthClaims;
use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatChoice, ChatChunkChoice, ChatChunkDelta, ChatCompletionChunk, ChatCompletionRequest,
    ChatCompletionResponse, ChatMessage, ChatUsage, LlmStreamResponse, SessionId, UserId,
};
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::feedback;

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    // ── Request validation ──────────────────────────────────────────
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

    // LLM01/LLM10: Input size validation
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

    // LLM10: Per-user concurrent request limiting
    let _request_guard = state.user_request_limiter.try_acquire(&claims.sub).map_err(|()| {
        ApiError(ThaiRagError::Validation(
            "Too many concurrent requests. Please wait for your previous request to complete.".into(),
        ))
    })?;

    // LLM10: Per-user token-bucket rate limiting
    if claims.sub != "anonymous" {
        state.user_rate_limiter.try_acquire(&claims.sub).map_err(|retry_after| {
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

    // Prepend history to messages if session exists
    let full_messages = if let Some(sid) = session_id {
        let mut msgs = state
            .session_store
            .get_history(&sid)
            .unwrap_or_default();
        msgs.extend(req.messages.clone());
        msgs
    } else {
        req.messages.clone()
    };

    // ── Scope resolution ────────────────────────────────────────────
    let user_id = if claims.sub == "anonymous" {
        None
    } else {
        claims.sub.parse::<Uuid>().ok().map(UserId)
    };

    let scope = if user_id.is_none() {
        AccessScope::unrestricted()
    } else {
        let uid = user_id.unwrap();
        let ws_ids = state.km_store.get_user_workspace_ids(uid);
        if ws_ids.is_empty() {
            AccessScope::none()
        } else {
            AccessScope::new(ws_ids)
        }
    };

    // ── Load conversation memories (Feature 1) ─────────────────────
    let memories = load_memories(&state, user_id);

    // ── Build available scopes for tool router (Feature 3) ─────────
    let available_scopes = build_searchable_scopes(&state, &scope);

    if req.stream {
        handle_stream(state, req, full_messages, scope, session_id, memories, available_scopes, user_id).await
    } else {
        handle_non_stream(state, req, full_messages, scope, session_id, memories, available_scopes, user_id).await
    }
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

/// Load conversation memory entries for a user from the KV store.
fn load_memories(state: &AppState, user_id: Option<UserId>) -> Vec<MemoryEntry> {
    let Some(uid) = user_id else { return vec![] };
    let key = format!("memory:{}", uid.0);
    state.km_store.get_setting(&key)
        .and_then(|json| serde_json::from_str::<Vec<MemoryEntry>>(&json).ok())
        .unwrap_or_default()
}

/// Save updated memories for a user.
fn save_memories(state: &AppState, user_id: UserId, memories: &[MemoryEntry], max: usize) {
    let mut entries = memories.to_vec();
    // Keep only the most recent N
    if entries.len() > max {
        entries.drain(..entries.len() - max);
    }
    let key = format!("memory:{}", user_id.0);
    if let Ok(json) = serde_json::to_string(&entries) {
        state.km_store.set_setting(&key, &json);
    }
}

/// Build searchable scopes from the user's accessible workspaces.
fn build_searchable_scopes(state: &AppState, scope: &AccessScope) -> Vec<SearchableScope> {
    if scope.is_unrestricted() {
        // For unrestricted access, list all workspaces
        state.km_store.list_workspaces_all()
            .into_iter()
            .map(|ws| SearchableScope {
                workspace_id: ws.id,
                name: ws.name,
                description: None,
            })
            .collect()
    } else {
        scope.workspace_ids.iter()
            .filter_map(|ws_id| {
                state.km_store.get_workspace(*ws_id).ok().map(|ws| SearchableScope {
                    workspace_id: ws.id,
                    name: ws.name,
                    description: None,
                })
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_non_stream(
    state: AppState,
    req: ChatCompletionRequest,
    full_messages: Vec<ChatMessage>,
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
) -> Result<Response, ApiError> {
    // Inject golden examples as few-shot demonstrations
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
        }];
        msgs.extend(full_messages.clone());
        msgs
    };

    let p = state.providers();
    let llm_resp = if let Some(ref pipeline) = p.chat_pipeline {
        pipeline.process(&augmented_messages, &scope, &memories, &available_scopes)
            .await.map_err(ApiError::from)?
    } else {
        p.orchestrator.process(&full_messages, &scope).await.map_err(ApiError::from)?
    };

    state
        .metrics
        .record_tokens(llm_resp.usage.prompt_tokens, llm_resp.usage.completion_tokens);
    persist_usage(&state, llm_resp.usage.prompt_tokens, llm_resp.usage.completion_tokens);

    // Save to session
    if let Some(sid) = session_id
        && let Some(last_user_msg) = req.messages.last().cloned()
    {
        let assistant_msg = ChatMessage {
            role: "assistant".to_string(),
            content: llm_resp.content.clone(),
        };
        state.session_store.append(sid, last_user_msg.clone(), assistant_msg.clone());

        // Feature 1: Async memory summarization
        if let Some(uid) = user_id {
            maybe_summarize_memory(
                state.clone(), p.chat_pipeline.clone(), uid, sid, memories,
            );
        }
    }

    let response_id = format!("chatcmpl-{}", Uuid::new_v4());

    // Feature 4: Store quality info for feedback correlation
    // (response_id is returned to client for feedback submission)

    let mut response = serde_json::to_value(ChatCompletionResponse {
        id: response_id,
        object: "chat.completion".to_string(),
        created: Utc::now().timestamp(),
        model: "ThaiRAG-1.0".to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: llm_resp.content,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: ChatUsage {
            prompt_tokens: llm_resp.usage.prompt_tokens,
            completion_tokens: llm_resp.usage.completion_tokens,
            total_tokens: llm_resp.usage.prompt_tokens + llm_resp.usage.completion_tokens,
        },
    })
    .unwrap();

    if let Some(sid) = session_id {
        response["session_id"] = serde_json::Value::String(sid.to_string());
    }

    Ok(Json(response).into_response())
}

/// Trigger async memory summarization if enough turns have accumulated.
fn maybe_summarize_memory(
    state: AppState,
    pipeline: Option<std::sync::Arc<thairag_agent::ChatPipeline>>,
    user_id: UserId,
    session_id: SessionId,
    existing_memories: Vec<MemoryEntry>,
) {
    let Some(pipeline) = pipeline else { return };
    let Some(memory_agent) = pipeline.conversation_memory() else { return };

    // Only summarize every 5 turns (10 messages)
    let history = state.session_store.get_history(&session_id);
    let msg_count = history.as_ref().map(|h| h.len()).unwrap_or(0);
    if msg_count < 10 || msg_count % 10 != 0 {
        return;
    }

    let messages = history.unwrap_or_default();
    let max_summaries = pipeline.conversation_memory()
        .map(|_| 10usize) // default
        .unwrap_or(10);

    // Clone what we need for the async task
    let state_clone = state.clone();
    let _ = memory_agent; // we'll re-access via pipeline in the task

    tokio::spawn(async move {
        let p = state_clone.providers();
        if let Some(ref pipeline) = p.chat_pipeline {
            if let Some(mem) = pipeline.conversation_memory() {
                match mem.summarize(&messages).await {
                    Ok(entry) => {
                        let mut all = existing_memories;
                        all.push(entry);
                        save_memories(&state_clone, user_id, &all, max_summaries);
                        tracing::debug!(user_id = %user_id.0, "Conversation memory saved");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to summarize conversation for memory");
                    }
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn handle_stream(
    state: AppState,
    req: ChatCompletionRequest,
    full_messages: Vec<ChatMessage>,
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
) -> Result<Response, ApiError> {
    let id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = Utc::now().timestamp();
    let model = "ThaiRAG-1.0".to_string();

    // Inject golden examples as few-shot demonstrations
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
        }];
        msgs.extend(full_messages.clone());
        msgs
    };

    let p = state.providers();
    let LlmStreamResponse {
        stream: token_stream,
        usage: usage_cell,
    } = if let Some(ref pipeline) = p.chat_pipeline {
        pipeline.process_stream(&augmented_messages, &scope, &memories, &available_scopes)
            .await.map_err(ApiError::from)?
    } else {
        p.orchestrator.process_stream(&augmented_messages, &scope).await.map_err(ApiError::from)?
    };

    let id_clone = id.clone();
    let model_clone = model.clone();
    let last_user_msg = req.messages.last().cloned();
    let pipeline_for_memory = p.chat_pipeline.clone();

    let sse_stream = async_stream::stream! {
        // First chunk: role
        let role_chunk = ChatCompletionChunk {
            id: id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model_clone.clone(),
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
        yield Ok::<_, std::convert::Infallible>(
            Event::default().data(serde_json::to_string(&role_chunk).unwrap())
        );

        // Content chunks
        let mut accumulated_content = String::new();
        let mut token_stream = std::pin::pin!(token_stream);
        while let Some(result) = token_stream.next().await {
            match result {
                Ok(token) => {
                    accumulated_content.push_str(&token);
                    let chunk = ChatCompletionChunk {
                        id: id_clone.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created,
                        model: model_clone.clone(),
                        choices: vec![ChatChunkChoice {
                            index: 0,
                            delta: ChatChunkDelta {
                                role: None,
                                content: Some(token),
                            },
                            finish_reason: None,
                        }],
                        usage: None,
                    };
                    yield Ok(Event::default().data(serde_json::to_string(&chunk).unwrap()));
                }
                Err(e) => {
                    let error_data = serde_json::json!({
                        "error": { "message": e.to_string(), "type": "stream_error" }
                    });
                    yield Ok(Event::default().data(serde_json::to_string(&error_data).unwrap()));
                    return;
                }
            }
        }

        // Save to session after stream completes
        if let Some(sid) = session_id
            && let Some(user_msg) = last_user_msg
        {
            let assistant_msg = ChatMessage {
                role: "assistant".to_string(),
                content: accumulated_content,
            };
            state.session_store.append(sid, user_msg, assistant_msg);

            // Feature 1: Async memory summarization
            if let Some(uid) = user_id {
                maybe_summarize_memory(
                    state.clone(), pipeline_for_memory, uid, sid, memories,
                );
            }
        }

        // Finish chunk
        let finish_chunk = ChatCompletionChunk {
            id: id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model_clone.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatChunkDelta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        };
        yield Ok(Event::default().data(serde_json::to_string(&finish_chunk).unwrap()));

        // Usage chunk
        let llm_usage = usage_cell.lock().unwrap().take().unwrap_or_default();
        state.metrics.record_tokens(llm_usage.prompt_tokens, llm_usage.completion_tokens);
        persist_usage(&state, llm_usage.prompt_tokens, llm_usage.completion_tokens);
        let usage_chunk = ChatCompletionChunk {
            id: id_clone.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model_clone.clone(),
            choices: vec![],
            usage: Some(ChatUsage {
                prompt_tokens: llm_usage.prompt_tokens,
                completion_tokens: llm_usage.completion_tokens,
                total_tokens: llm_usage.prompt_tokens + llm_usage.completion_tokens,
            }),
        };
        yield Ok(Event::default().data(serde_json::to_string(&usage_chunk).unwrap()));

        // [DONE] sentinel
        yield Ok(Event::default().data("[DONE]"));
    };

    Ok(Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}
