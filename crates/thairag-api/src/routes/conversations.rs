//! First-party chat-UI conversation history.
//!
//! Durable, per-user chat conversations for the native ThaiRAG chat frontend,
//! exposed under `/api/chat/conversations`. This is the persistence/CRUD layer
//! (Phase 1); the streaming endpoint that *writes* assistant turns lands in
//! Phase 2. Every route enforces per-user ownership via the JWT `sub` claim —
//! a conversation is only visible to the user that created it.
//!
//! Note: distinct from the ephemeral in-memory `SessionStore` used by the
//! OpenAI-compatible `/v1` surface (OWUI and external clients).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{ConversationRow, MessageRow};

// ── Request types ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateConversationRequest {
    #[serde(default)]
    pub title: Option<String>,
    /// Optional hard scope (workspace id) to pin this conversation to.
    #[serde(default)]
    pub workspace_scope: Option<String>,
}

#[derive(Deserialize)]
pub struct RenameConversationRequest {
    pub title: String,
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Resolve the requesting user id, requiring a real signed-in user. API-key /
/// anonymous principals (whose `sub` is not a UUID) are rejected — chat
/// conversations are always owned by a concrete user.
fn current_user_id(claims: &AuthClaims) -> Result<String, ApiError> {
    if claims.sub.parse::<uuid::Uuid>().is_ok() {
        Ok(claims.sub.clone())
    } else {
        Err(ApiError(ThaiRagError::Auth(
            "A signed-in user is required for chat conversations".into(),
        )))
    }
}

/// Fetch a conversation and assert the requester owns it. Returns 404 when it
/// does not exist and 403 when it belongs to a different user — never leaking
/// existence across users.
fn require_owned(
    state: &AppState,
    claims: &AuthClaims,
    conversation_id: &str,
) -> Result<ConversationRow, ApiError> {
    let user_id = current_user_id(claims)?;
    let conv = state
        .km_store
        .get_conversation(conversation_id)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Conversation not found".into())))?;
    if conv.user_id != user_id {
        return Err(ApiError(ThaiRagError::Authorization(
            "You do not have access to this conversation".into(),
        )));
    }
    Ok(conv)
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/chat/conversations — list the current user's conversations.
pub async fn list_conversations(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
) -> Result<Json<Vec<ConversationRow>>, ApiError> {
    let user_id = current_user_id(&claims)?;
    Ok(Json(state.km_store.list_conversations(&user_id)))
}

/// POST /api/chat/conversations — create a new conversation.
pub async fn create_conversation(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    AppJson(body): AppJson<CreateConversationRequest>,
) -> Result<(StatusCode, Json<ConversationRow>), ApiError> {
    let user_id = current_user_id(&claims)?;
    let title = body.title.unwrap_or_default();
    let conv =
        state
            .km_store
            .create_conversation(&user_id, &title, body.workspace_scope.as_deref())?;
    Ok((StatusCode::CREATED, Json(conv)))
}

/// GET /api/chat/conversations/{id} — fetch one conversation (owner only).
pub async fn get_conversation(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<Json<ConversationRow>, ApiError> {
    let conv = require_owned(&state, &claims, &id)?;
    Ok(Json(conv))
}

/// PATCH /api/chat/conversations/{id} — rename a conversation (owner only).
pub async fn rename_conversation(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(id): Path<String>,
    AppJson(body): AppJson<RenameConversationRequest>,
) -> Result<Json<ConversationRow>, ApiError> {
    require_owned(&state, &claims, &id)?;
    if body.title.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "Title cannot be empty".into(),
        )));
    }
    state.km_store.rename_conversation(&id, &body.title)?;
    let conv = state
        .km_store
        .get_conversation(&id)
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Conversation not found".into())))?;
    Ok(Json(conv))
}

/// DELETE /api/chat/conversations/{id} — delete a conversation + its messages.
pub async fn delete_conversation(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_owned(&state, &claims, &id)?;
    state.km_store.delete_conversation(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/chat/conversations/{id}/messages — list a conversation's messages.
pub async fn list_messages(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<Json<Vec<MessageRow>>, ApiError> {
    require_owned(&state, &claims, &id)?;
    Ok(Json(state.km_store.list_messages(&id)))
}
