use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatChoice, ChatChunkChoice, ChatChunkDelta, ChatCompletionChunk, ChatCompletionRequest,
    ChatCompletionResponse, ChatMessage, ChatUsage, LlmStreamResponse, UserId,
};

use crate::app_state::AppState;
use crate::error::ApiError;

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    let scope = if claims.sub == "anonymous" {
        AccessScope::unrestricted()
    } else {
        let user_id = claims
            .sub
            .parse::<Uuid>()
            .map(UserId)
            .map_err(|_| ApiError(thairag_core::ThaiRagError::Auth("Invalid user ID".into())))?;
        let ws_ids = state.km_store.get_user_workspace_ids(user_id);
        if ws_ids.is_empty() {
            AccessScope::unrestricted()
        } else {
            AccessScope::new(ws_ids)
        }
    };

    if req.stream {
        handle_stream(state, req, scope).await
    } else {
        handle_non_stream(state, req, scope).await
    }
}

async fn handle_non_stream(
    state: AppState,
    req: ChatCompletionRequest,
    scope: AccessScope,
) -> Result<Response, ApiError> {
    let llm_resp = state
        .orchestrator
        .process(&req.messages, &scope)
        .await
        .map_err(ApiError::from)?;

    let response = ChatCompletionResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
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
    };

    Ok(Json(response).into_response())
}

async fn handle_stream(
    state: AppState,
    req: ChatCompletionRequest,
    scope: AccessScope,
) -> Result<Response, ApiError> {
    let id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = Utc::now().timestamp();
    let model = "ThaiRAG-1.0".to_string();

    let LlmStreamResponse { stream: token_stream, usage: usage_cell } = state
        .orchestrator
        .process_stream(&req.messages, &scope)
        .await
        .map_err(ApiError::from)?;

    let id_clone = id.clone();
    let model_clone = model.clone();

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
        let mut token_stream = std::pin::pin!(token_stream);
        while let Some(result) = token_stream.next().await {
            match result {
                Ok(token) => {
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

        // Usage chunk (matches OpenAI stream_options.include_usage wire format)
        let llm_usage = usage_cell.lock().unwrap().take().unwrap_or_default();
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
        yield Ok(Event::default().data("[DONE]".to_string()));
    };

    Ok(Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}
