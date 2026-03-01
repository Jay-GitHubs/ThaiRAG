use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatChoice, ChatChunkChoice, ChatChunkDelta, ChatCompletionChunk, ChatCompletionRequest,
    ChatCompletionResponse, ChatMessage, ChatUsage,
};

use crate::app_state::AppState;
use crate::error::ApiError;

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    let scope = AccessScope::unrestricted();

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
    let response_text = state
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
                content: response_text,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: ChatUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
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

    let token_stream = state
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
        };
        yield Ok(Event::default().data(serde_json::to_string(&finish_chunk).unwrap()));

        // [DONE] sentinel
        yield Ok(Event::default().data("[DONE]".to_string()));
    };

    Ok(Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}
