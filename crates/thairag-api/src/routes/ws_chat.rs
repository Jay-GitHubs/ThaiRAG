use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_agent::conversation_memory::MemoryEntry;
use thairag_agent::tool_router::SearchableScope;
use thairag_auth::AuthClaims;
use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatMessage, LlmStreamResponse, MetadataCell, PipelineMetadata, SessionId, UserId,
};

use crate::app_state::AppState;
use crate::routes::chat::build_searchable_scopes;
use crate::routes::feedback;

/// Keep-alive ping interval.
const PING_INTERVAL: Duration = Duration::from_secs(30);

// ── WebSocket protocol types ────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsClientMessage {
    Chat {
        session_id: Option<String>,
        messages: Vec<ChatMessage>,
        #[serde(default = "default_true")]
        stream: bool,
    },
    Pong,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsServerMessage {
    Chunk {
        content: String,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Done,
    Error {
        message: String,
    },
    Ping,
}

// ── Handler: upgrade HTTP to WebSocket ──────────────────────────────

/// WebSocket chat endpoint. Auth is validated on the upgrade request
/// via the same `?token=` query param mechanism used by SSE, or via
/// Authorization header / X-API-Key.
///
/// The auth middleware runs before this handler, so `AuthClaims` is
/// already in extensions by the time we get here.
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<AuthClaims>,
    headers: axum::http::HeaderMap,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state, claims, headers))
}

// ── Main connection loop ────────────────────────────────────────────

async fn handle_ws_connection(
    socket: WebSocket,
    state: AppState,
    claims: AuthClaims,
    headers: axum::http::HeaderMap,
) {
    handle_ws_loop(socket, state, claims, headers).await;
}

async fn handle_ws_loop(
    mut socket: WebSocket,
    state: AppState,
    claims: AuthClaims,
    headers: axum::http::HeaderMap,
) {
    let mut ping_interval = tokio::time::interval(PING_INTERVAL);

    loop {
        tokio::select! {
            // Keep-alive ping
            _ = ping_interval.tick() => {
                let msg = serde_json::to_string(&WsServerMessage::Ping).unwrap();
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            // Incoming message
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let client_msg: WsClientMessage = match serde_json::from_str(&text) {
                            Ok(m) => m,
                            Err(e) => {
                                let err_msg = serde_json::to_string(&WsServerMessage::Error {
                                    message: format!("Invalid JSON: {e}"),
                                }).unwrap();
                                if socket.send(Message::Text(err_msg.into())).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                        };
                        match client_msg {
                            WsClientMessage::Chat { session_id, messages, stream } => {
                                handle_chat_message(
                                    &state, &claims, &headers, &mut socket,
                                    session_id, messages, stream,
                                ).await;
                            }
                            WsClientMessage::Pong => {
                                // Client responded to our ping; nothing to do.
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {} // Binary, Ping/Pong handled by axum
                }
            }
        }
    }

    tracing::debug!(user = %claims.sub, "WebSocket connection closed");
}

// ── Chat message processing ─────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_chat_message(
    state: &AppState,
    claims: &AuthClaims,
    headers: &axum::http::HeaderMap,
    socket: &mut WebSocket,
    session_id_str: Option<String>,
    messages: Vec<ChatMessage>,
    stream: bool,
) {
    // ── Validation ──────────────────────────────────────────────────
    if messages.is_empty() {
        let _ = send_msg(
            socket,
            &WsServerMessage::Error {
                message: "messages must not be empty".into(),
            },
        )
        .await;
        return;
    }

    let max_messages = state.config.server.max_chat_messages;
    if messages.len() > max_messages {
        let _ = send_msg(
            socket,
            &WsServerMessage::Error {
                message: format!("too many messages: {} (max {max_messages})", messages.len()),
            },
        )
        .await;
        return;
    }

    let max_msg_len = state.config.server.max_message_length;
    for (i, msg) in messages.iter().enumerate() {
        if msg.content.len() > max_msg_len {
            let _ = send_msg(
                socket,
                &WsServerMessage::Error {
                    message: format!(
                        "message[{i}] content too long: {} chars (max {max_msg_len})",
                        msg.content.len()
                    ),
                },
            )
            .await;
            return;
        }
    }

    // ── Per-user rate limiting ───────────────────────────────────────
    let _request_guard = match state.user_request_limiter.try_acquire(&claims.sub) {
        Ok(g) => g,
        Err(()) => {
            let _ = send_msg(
                socket,
                &WsServerMessage::Error {
                    message: "Too many concurrent requests. Please wait for your previous request to complete.".into(),
                },
            )
            .await;
            return;
        }
    };

    if claims.sub != "anonymous"
        && let Err(retry_after) = state.user_rate_limiter.try_acquire(&claims.sub)
    {
        let _ = send_msg(
            socket,
            &WsServerMessage::Error {
                message: format!(
                    "User rate limit exceeded. Retry after {:.0} seconds.",
                    retry_after.ceil()
                ),
            },
        )
        .await;
        return;
    }

    // ── Session handling ────────────────────────────────────────────
    let session_id = match &session_id_str {
        Some(id_str) => match id_str.parse::<Uuid>() {
            Ok(uuid) => Some(SessionId(uuid)),
            Err(_) => {
                let _ = send_msg(
                    socket,
                    &WsServerMessage::Error {
                        message: format!("invalid session_id: {id_str}"),
                    },
                )
                .await;
                return;
            }
        },
        None => None,
    };

    // Prepend history
    let full_messages = if let Some(sid) = session_id {
        let mut msgs = state
            .session_store
            .get_history(&sid)
            .await
            .unwrap_or_default();
        msgs.extend(messages.clone());
        msgs
    } else {
        messages.clone()
    };

    // ── Scope resolution (same as REST handler) ─────────────────────
    let user_id = resolve_user_id(state, claims, headers);
    let scope = resolve_scope(state, claims, user_id);
    let settings_scope = scope
        .workspace_ids
        .first()
        .map(|ws_id| state.resolve_scope_for_workspace(*ws_id))
        .unwrap_or(crate::store::SettingsScope::Global);

    // ── Load memories ───────────────────────────────────────────────
    let memories = super::chat::load_memories(state, user_id);

    // ── Context compaction ──────────────────────────────────────────
    let full_messages =
        super::chat::maybe_compact_context(state, full_messages, session_id, user_id).await;

    // ── Personal memory retrieval ───────────────────────────────────
    let personal_memories =
        super::chat::retrieve_personal_memories(state, user_id, &full_messages).await;

    // ── Build scopes ────────────────────────────────────────────────
    let available_scopes = build_searchable_scopes(state, &scope);

    // ── Inject personal memory context ──────────────────────────────
    let full_messages =
        super::chat::inject_personal_memory_context(full_messages, &personal_memories);

    // ── Inject golden examples ──────────────────────────────────────
    let golden = feedback::load_golden_examples_for_workspace(state, None);
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

    // ── Execute pipeline ────────────────────────────────────────────
    if stream {
        handle_ws_stream(
            state,
            socket,
            augmented_messages,
            &messages,
            scope,
            session_id,
            memories,
            available_scopes,
            user_id,
            settings_scope,
        )
        .await;
    } else {
        handle_ws_non_stream(
            state,
            socket,
            augmented_messages,
            &messages,
            scope,
            session_id,
            memories,
            available_scopes,
            user_id,
            settings_scope,
        )
        .await;
    }
}

// ── Streaming response over WebSocket ───────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_ws_stream(
    state: &AppState,
    socket: &mut WebSocket,
    augmented_messages: Vec<ChatMessage>,
    original_messages: &[ChatMessage],
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
    settings_scope: crate::store::SettingsScope,
) {
    let p = state.providers();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();

    let stream_result = if let Some(ref pipeline) = scoped_pipeline {
        pipeline
            .process_stream(
                &augmented_messages,
                &scope,
                &memories,
                &available_scopes,
                Some(progress_tx),
                Some(metadata_cell.clone()),
            )
            .await
    } else {
        drop(progress_tx);
        p.orchestrator
            .process_stream(&augmented_messages, &scope)
            .await
    };

    let LlmStreamResponse {
        stream: token_stream,
        usage: usage_cell,
    } = match stream_result {
        Ok(resp) => resp,
        Err(e) => {
            let _ = send_msg(
                socket,
                &WsServerMessage::Error {
                    message: e.to_string(),
                },
            )
            .await;
            return;
        }
    };

    // Stream content chunks
    let mut accumulated = String::new();
    let mut token_stream = std::pin::pin!(token_stream);
    while let Some(result) = token_stream.next().await {
        match result {
            Ok(token) => {
                accumulated.push_str(&token);
                if send_msg(socket, &WsServerMessage::Chunk { content: token })
                    .await
                    .is_err()
                {
                    return; // connection lost
                }
            }
            Err(e) => {
                let _ = send_msg(
                    socket,
                    &WsServerMessage::Error {
                        message: e.to_string(),
                    },
                )
                .await;
                return;
            }
        }
    }

    // Usage
    let llm_usage = usage_cell.lock().unwrap().take().unwrap_or_default();
    state
        .metrics
        .record_tokens(llm_usage.prompt_tokens, llm_usage.completion_tokens);
    super::chat::persist_usage(state, llm_usage.prompt_tokens, llm_usage.completion_tokens);

    let _ = send_msg(
        socket,
        &WsServerMessage::Usage {
            prompt_tokens: llm_usage.prompt_tokens,
            completion_tokens: llm_usage.completion_tokens,
        },
    )
    .await;

    // Save to session
    if let Some(sid) = session_id
        && let Some(last_user_msg) = original_messages.last().cloned()
    {
        let assistant_msg = ChatMessage {
            role: "assistant".to_string(),
            content: accumulated,
        };
        state
            .session_store
            .append(sid, last_user_msg, assistant_msg, user_id)
            .await;
    }

    let _ = send_msg(socket, &WsServerMessage::Done).await;
}

// ── Non-streaming response over WebSocket ───────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_ws_non_stream(
    state: &AppState,
    socket: &mut WebSocket,
    augmented_messages: Vec<ChatMessage>,
    original_messages: &[ChatMessage],
    scope: AccessScope,
    session_id: Option<SessionId>,
    memories: Vec<MemoryEntry>,
    available_scopes: Vec<SearchableScope>,
    user_id: Option<UserId>,
    settings_scope: crate::store::SettingsScope,
) {
    let p = state.providers();
    let metadata_cell: MetadataCell = Arc::new(Mutex::new(PipelineMetadata::default()));
    let scoped_pipeline = state.get_scoped_pipeline(&settings_scope);

    let (progress_tx, _progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<thairag_core::types::PipelineProgress>();

    let result = if let Some(ref pipeline) = scoped_pipeline {
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
    } else {
        drop(progress_tx);
        p.orchestrator.process(&augmented_messages, &scope).await
    };

    match result {
        Ok(llm_resp) => {
            state.metrics.record_tokens(
                llm_resp.usage.prompt_tokens,
                llm_resp.usage.completion_tokens,
            );
            super::chat::persist_usage(
                state,
                llm_resp.usage.prompt_tokens,
                llm_resp.usage.completion_tokens,
            );

            // Send full content as a single chunk
            let _ = send_msg(
                socket,
                &WsServerMessage::Chunk {
                    content: llm_resp.content.clone(),
                },
            )
            .await;
            let _ = send_msg(
                socket,
                &WsServerMessage::Usage {
                    prompt_tokens: llm_resp.usage.prompt_tokens,
                    completion_tokens: llm_resp.usage.completion_tokens,
                },
            )
            .await;

            // Save to session
            if let Some(sid) = session_id
                && let Some(last_user_msg) = original_messages.last().cloned()
            {
                let assistant_msg = ChatMessage {
                    role: "assistant".to_string(),
                    content: llm_resp.content,
                };
                state
                    .session_store
                    .append(sid, last_user_msg, assistant_msg, user_id)
                    .await;
            }

            let _ = send_msg(socket, &WsServerMessage::Done).await;
        }
        Err(e) => {
            let _ = send_msg(
                socket,
                &WsServerMessage::Error {
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

async fn send_msg(socket: &mut WebSocket, msg: &WsServerMessage) -> Result<(), ()> {
    let json = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}

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

fn resolve_scope(state: &AppState, claims: &AuthClaims, user_id: Option<UserId>) -> AccessScope {
    if let Some(uid) = user_id {
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
    }
}
