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
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::{DocId, UserId};

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
    /// Chat mode: `rag` (default, knowledge-base retrieval) or `general`
    /// (non-RAG plain assistant). Invalid values fall back to `rag`.
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Deserialize)]
pub struct RenameConversationRequest {
    pub title: String,
}

#[derive(Deserialize)]
pub struct MessageFeedbackRequest {
    /// 1 = thumbs up, -1 = thumbs down, 0 = clear. Clamped server-side.
    pub feedback: i32,
}

/// A workspace the signed-in user can search, for the chat scope picker.
#[derive(Serialize)]
pub struct WorkspaceOption {
    pub id: String,
    pub name: String,
}

/// Chat capabilities the first-party UI reads to decide which affordances to
/// show (e.g. the General-mode toggle, the image-generation button).
#[derive(Serialize)]
pub struct ChatFeatures {
    pub general_chat_enabled: bool,
    pub image_generation_enabled: bool,
}

/// GET /api/chat/features — feature flags for the chat client.
pub async fn chat_features(State(state): State<AppState>) -> Json<ChatFeatures> {
    let gc = crate::routes::settings::build_effective_general_chat(&state.config, &*state.km_store);
    Json(ChatFeatures {
        general_chat_enabled: gc.enabled,
        image_generation_enabled: gc.image_generation.enabled,
    })
}

/// A cited document's content for the in-app source viewer.
#[derive(Serialize)]
pub struct DocumentSource {
    pub doc_id: String,
    pub title: String,
    pub mime_type: String,
    /// Converted/extracted text of the document (the in-app viewer renders this
    /// and highlights the cited passage). Phase 2 will add original-file render.
    pub content: String,
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
    // `general` is allowed only when general chat is enabled; anything else → `rag`.
    let gc_enabled =
        crate::routes::settings::build_effective_general_chat(&state.config, &*state.km_store)
            .enabled;
    let mode = match body.mode.as_deref() {
        Some("general") if gc_enabled => "general",
        _ => "rag",
    };
    let conv = state.km_store.create_conversation(
        &user_id,
        &title,
        body.workspace_scope.as_deref(),
        mode,
    )?;
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

/// POST /api/chat/conversations/{id}/messages/{message_id}/feedback — set a
/// thumbs rating on an assistant message. `feedback`: 1 = up, -1 = down, 0 =
/// clear. Scoped to a conversation the caller owns; unknown message → 404.
pub async fn set_message_feedback(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path((id, message_id)): Path<(String, String)>,
    AppJson(body): AppJson<MessageFeedbackRequest>,
) -> Result<StatusCode, ApiError> {
    require_owned(&state, &claims, &id)?;
    let fb = body.feedback.clamp(-1, 1);
    let updated = state.km_store.set_message_feedback(&id, &message_id, fb)?;
    if updated == 0 {
        return Err(ApiError(ThaiRagError::NotFound(
            "message not found in conversation".into(),
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/chat/documents/{doc_id}/source — fetch a cited document's converted
/// text for the in-app source viewer. Permission-checked: the document must live
/// in a workspace the signed-in user can search (the same scope retrieval uses),
/// so this never widens access beyond what produced the citation.
pub async fn get_document_source(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(doc_id): Path<String>,
) -> Result<Json<DocumentSource>, ApiError> {
    let user_id = current_user_id(&claims)?;
    let uid = UserId(
        user_id
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Auth("Invalid user id in token".into())))?,
    );
    let did = DocId(
        doc_id
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Validation("invalid document id".into())))?,
    );
    let doc = state
        .km_store
        .get_document(did)
        .map_err(|_| ApiError(ThaiRagError::NotFound("document not found".into())))?;
    if !state
        .km_store
        .get_user_workspace_ids(uid)
        .contains(&doc.workspace_id)
    {
        return Err(ApiError(ThaiRagError::Authorization(
            "You do not have access to this document".into(),
        )));
    }
    let content = state
        .km_store
        .get_document_content(did)
        .ok()
        .flatten()
        .unwrap_or_default();
    Ok(Json(DocumentSource {
        doc_id: doc.id.0.to_string(),
        title: doc.title,
        mime_type: doc.mime_type,
        content,
    }))
}

/// GET /api/chat/documents/{doc_id}/original — stream the original uploaded file
/// bytes (e.g. the source PDF for the in-app PDF viewer). Same owner/scope check
/// as the source-text route. 404 if the document kept no original bytes.
pub async fn get_document_original(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
    Path(doc_id): Path<String>,
) -> Result<Response, ApiError> {
    let user_id = current_user_id(&claims)?;
    let uid = UserId(
        user_id
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Auth("Invalid user id in token".into())))?,
    );
    let did = DocId(
        doc_id
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Validation("invalid document id".into())))?,
    );
    let doc = state
        .km_store
        .get_document(did)
        .map_err(|_| ApiError(ThaiRagError::NotFound("document not found".into())))?;
    if !state
        .km_store
        .get_user_workspace_ids(uid)
        .contains(&doc.workspace_id)
    {
        return Err(ApiError(ThaiRagError::Authorization(
            "You do not have access to this document".into(),
        )));
    }
    let bytes = state
        .km_store
        .get_document_file(did)?
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("no original file stored".into())))?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, doc.mime_type),
            (header::CACHE_CONTROL, "private, max-age=300".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// GET /api/chat/workspaces — the workspaces the user can search, for the chat
/// scope picker. Mirrors the retrieval scope (their permissioned workspaces).
pub async fn list_workspaces(
    State(state): State<AppState>,
    claims: axum::Extension<AuthClaims>,
) -> Result<Json<Vec<WorkspaceOption>>, ApiError> {
    let user_id = current_user_id(&claims)?;
    let uid = UserId(
        user_id
            .parse()
            .map_err(|_| ApiError(ThaiRagError::Auth("Invalid user id in token".into())))?,
    );
    let opts: Vec<WorkspaceOption> = state
        .km_store
        .get_user_workspace_ids(uid)
        .into_iter()
        .filter_map(|wid| {
            state
                .km_store
                .get_workspace(wid)
                .ok()
                .map(|w| WorkspaceOption {
                    id: w.id.0.to_string(),
                    name: w.name,
                })
        })
        .collect();
    Ok(Json(opts))
}
