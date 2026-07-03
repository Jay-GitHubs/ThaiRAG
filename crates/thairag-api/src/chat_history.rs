//! Conversation-history glue for the first-party chat UI.
//!
//! Bridges the durable `conversations`/`messages` store (PR-1) and the chat
//! pipeline: owner-checked loading of stored history into pipeline
//! [`ChatMessage`]s, and persistence of one completed turn (user prompt +
//! assistant reply with its citations and token stats).
//!
//! This is consumed by the Phase 2 `/api/chat` streaming endpoint. It is
//! deliberately independent of the `/v1` ephemeral `SessionStore` path — that
//! path is left untouched for OWUI / OpenAI-compatible clients.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thairag_core::ThaiRagError;
use thairag_core::types::ChatMessage;

use crate::store::{KmStoreTrait, MessageRow};

type Result<T> = std::result::Result<T, ThaiRagError>;

/// Default number of most-recent prior messages loaded into the model context.
/// Mirrors the legacy `SessionStore` cap so behaviour is consistent across the
/// two history backends.
pub const DEFAULT_HISTORY_LIMIT: usize = 50;

/// Why a conversation could not be accessed for the requesting user. Maps to
/// HTTP 404 / 403 at the route layer without leaking existence across users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationAccess {
    /// No conversation with that id exists.
    NotFound,
    /// The conversation exists but belongs to a different user.
    Forbidden,
}

/// A citation persisted alongside an assistant message, in the shape the chat
/// UI renders directly. Stored as a JSON array in `messages.citations`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedCitation {
    pub doc_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Optional signed viewer link (the Phase 2 citation URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Snippet of the cited chunk's text, used by the in-app source viewer to
    /// locate and highlight the passage within the full document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Token accounting persisted alongside an assistant message. Stored as a JSON
/// object in `messages.token_stats`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PersistedTokenStats {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

/// A source image persisted alongside an assistant message, in the shape the
/// chat UI renders inline. Stored as a JSON array in `messages.images`. `url`
/// points at the token-gated media route so a browser `<img>` can load it
/// without an auth header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedImage {
    pub image_id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
}

/// Convert stored message rows into pipeline messages.
///
/// Only `role` + `content` are replayed. Images are intentionally *not*
/// re-attached from history: the pipeline re-hydrates source images from the
/// corpus for the *current* turn, and replaying old base64 blobs would bloat
/// the context window for no retrieval benefit.
pub fn rows_to_chat_messages(rows: &[MessageRow]) -> Vec<ChatMessage> {
    rows.iter()
        .map(|m| ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            images: Vec::new(),
        })
        .collect()
}

/// Load a conversation's history as pipeline messages, enforcing ownership.
///
/// Returns at most `limit` of the most-recent messages, in chronological
/// order. The ownership check here is the single source of truth the caller
/// can rely on before persisting a turn — a successful return means the
/// requester owns the conversation.
pub fn load_history(
    store: &Arc<dyn KmStoreTrait>,
    conversation_id: &str,
    user_id: &str,
    limit: usize,
) -> std::result::Result<Vec<ChatMessage>, ConversationAccess> {
    let conv = store
        .get_conversation(conversation_id)
        .ok_or(ConversationAccess::NotFound)?;
    if conv.user_id != user_id {
        return Err(ConversationAccess::Forbidden);
    }
    let mut rows = store.list_messages(conversation_id);
    if rows.len() > limit {
        // Keep the most-recent `limit` messages (list is chronological ASC).
        rows = rows.split_off(rows.len() - limit);
    }
    Ok(rows_to_chat_messages(&rows))
}

/// Metadata for a user upload persisted with its message. The file content is
/// session-scoped (used as answer context, never stored in the KB); only this
/// metadata survives, so the UI can keep showing the attachment chips after a
/// reload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistedAttachment {
    pub name: String,
    pub mime: String,
    pub size: usize,
}

/// Persist one completed turn: the user's prompt and the assistant's reply
/// (with its citations and token stats). Bumps the conversation's `updated_at`
/// via the underlying store. Ownership MUST already have been verified by the
/// caller (e.g. via [`load_history`]). Returns the stored assistant row.
#[allow(clippy::too_many_arguments)]
pub fn persist_turn(
    store: &Arc<dyn KmStoreTrait>,
    conversation_id: &str,
    user_content: &str,
    assistant_content: &str,
    citations: &[PersistedCitation],
    images: &[PersistedImage],
    token_stats: &PersistedTokenStats,
    attachments: &[PersistedAttachment],
) -> Result<MessageRow> {
    let attachments_json = serde_json::to_string(attachments).unwrap_or_else(|_| "[]".to_string());
    store.append_message(
        conversation_id,
        "user",
        user_content,
        "[]",
        "[]",
        "{}",
        &attachments_json,
    )?;
    persist_assistant(
        store,
        conversation_id,
        assistant_content,
        citations,
        images,
        token_stats,
    )
}

/// Persist only an assistant message (no user turn). Used by regenerate, where
/// the user turn already exists and only the answer is being replaced.
pub fn persist_assistant(
    store: &Arc<dyn KmStoreTrait>,
    conversation_id: &str,
    assistant_content: &str,
    citations: &[PersistedCitation],
    images: &[PersistedImage],
    token_stats: &PersistedTokenStats,
) -> Result<MessageRow> {
    let citations_json = serde_json::to_string(citations).unwrap_or_else(|_| "[]".to_string());
    let images_json = serde_json::to_string(images).unwrap_or_else(|_| "[]".to_string());
    let token_json = serde_json::to_string(token_stats).unwrap_or_else(|_| "{}".to_string());
    store.append_message(
        conversation_id,
        "assistant",
        assistant_content,
        &citations_json,
        &images_json,
        &token_json,
        "[]",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::memory::MemoryKmStore;

    fn store() -> Arc<dyn KmStoreTrait> {
        Arc::new(MemoryKmStore::new())
    }

    const ALICE: &str = "11111111-1111-1111-1111-111111111111";
    const BOB: &str = "22222222-2222-2222-2222-222222222222";

    #[test]
    fn load_history_owner_returns_chronological() {
        let store = store();
        let conv = store.create_conversation(ALICE, "T", None, "rag").unwrap();
        store
            .append_message(&conv.id, "user", "q1", "[]", "[]", "{}", "[]")
            .unwrap();
        store
            .append_message(&conv.id, "assistant", "a1", "[]", "[]", "{}", "[]")
            .unwrap();

        let msgs = load_history(&store, &conv.id, ALICE, DEFAULT_HISTORY_LIMIT).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "q1");
        assert_eq!(msgs[1].role, "assistant");
        // Images are not replayed from history.
        assert!(msgs[1].images.is_empty());
    }

    #[test]
    fn load_history_respects_limit_keeping_most_recent() {
        let store = store();
        let conv = store.create_conversation(ALICE, "T", None, "rag").unwrap();
        for i in 0..5 {
            store
                .append_message(&conv.id, "user", &format!("m{i}"), "[]", "[]", "{}", "[]")
                .unwrap();
        }
        let msgs = load_history(&store, &conv.id, ALICE, 2).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "m3");
        assert_eq!(msgs[1].content, "m4");
    }

    #[test]
    fn load_history_unknown_is_not_found() {
        let store = store();
        assert_eq!(
            load_history(&store, "nope", ALICE, 10).unwrap_err(),
            ConversationAccess::NotFound
        );
    }

    #[test]
    fn load_history_cross_user_is_forbidden() {
        let store = store();
        let conv = store.create_conversation(ALICE, "T", None, "rag").unwrap();
        assert_eq!(
            load_history(&store, &conv.id, BOB, 10).unwrap_err(),
            ConversationAccess::Forbidden
        );
    }

    #[test]
    fn persist_turn_writes_user_and_assistant_with_metadata() {
        let store = store();
        let conv = store.create_conversation(ALICE, "T", None, "rag").unwrap();
        let citations = vec![PersistedCitation {
            doc_id: "d1".into(),
            title: "Manual".into(),
            page: Some(4),
            section: Some("Setup".into()),
            url: None,
            snippet: None,
        }];
        let stats = PersistedTokenStats {
            prompt_tokens: 12,
            completion_tokens: 7,
        };
        let images = vec![PersistedImage {
            image_id: "img-1".into(),
            url: "https://h/api/chat/media/img-1?token=t".into(),
            page: Some(4),
        }];

        let assistant = persist_turn(
            &store,
            &conv.id,
            "how?",
            "do X",
            &citations,
            &images,
            &stats,
            &[],
        )
        .unwrap();
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content, "do X");

        let rows = store.list_messages(&conv.id);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, "user");
        assert_eq!(rows[0].content, "how?");

        // Metadata round-trips through the JSON columns.
        let parsed: Vec<PersistedCitation> = serde_json::from_str(&rows[1].citations).unwrap();
        assert_eq!(parsed, citations);
        let parsed_stats: PersistedTokenStats = serde_json::from_str(&rows[1].token_stats).unwrap();
        assert_eq!(parsed_stats, stats);
        let parsed_images: Vec<PersistedImage> = serde_json::from_str(&rows[1].images).unwrap();
        assert_eq!(parsed_images, images);
    }

    #[test]
    fn persisted_citation_omits_empty_optionals() {
        let c = PersistedCitation {
            doc_id: "d".into(),
            title: "t".into(),
            page: None,
            section: None,
            url: None,
            snippet: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, r#"{"doc_id":"d","title":"t"}"#);
    }
}
