use axum::extract::State;
use axum::Json;
use chrono::Utc;
use uuid::Uuid;

use thairag_core::permission::AccessScope;
use thairag_core::types::{
    ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatUsage,
};

use crate::app_state::AppState;
use crate::error::ApiError;

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, ApiError> {
    // For now, use unrestricted scope (auth will refine this later)
    let scope = AccessScope::unrestricted();

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

    Ok(Json(response))
}
