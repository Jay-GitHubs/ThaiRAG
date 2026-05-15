use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use thairag_core::traits::SessionStoreTrait;
use thairag_core::types::{ChatMessage, SessionAttachment, SessionId, UserId};

const MAX_HISTORY: usize = 50;

struct Session {
    messages: Vec<ChatMessage>,
    user_id: Option<UserId>,
    #[allow(dead_code)]
    created_at: Instant,
    updated_at: Instant,
    /// Conversation summary (populated by auto-summarization or manual trigger).
    summary: Option<String>,
    /// Number of messages in the session when the summary was last generated.
    summary_message_count: usize,
    /// Per-request document attachments active for this session.
    attachments: Vec<SessionAttachment>,
}

#[derive(Clone)]
pub struct InMemorySessionStore {
    sessions: std::sync::Arc<DashMap<SessionId, Session>>,
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: std::sync::Arc::new(DashMap::new()),
        }
    }
}

#[async_trait]
impl SessionStoreTrait for InMemorySessionStore {
    async fn get_history(&self, id: &SessionId) -> Option<Vec<ChatMessage>> {
        self.sessions.get(id).map(|s| s.messages.clone())
    }

    async fn append(
        &self,
        id: SessionId,
        user_msg: ChatMessage,
        assistant_msg: ChatMessage,
        user_id: Option<UserId>,
    ) {
        let now = Instant::now();
        let mut entry = self.sessions.entry(id).or_insert_with(|| Session {
            messages: Vec::new(),
            user_id,
            created_at: now,
            updated_at: now,
            summary: None,
            summary_message_count: 0,
            attachments: Vec::new(),
        });
        let session = entry.value_mut();
        session.messages.push(user_msg);
        session.messages.push(assistant_msg);
        session.updated_at = now;

        // Trim oldest messages if over cap
        if session.messages.len() > MAX_HISTORY {
            let drain_count = session.messages.len() - MAX_HISTORY;
            session.messages.drain(..drain_count);
        }
    }

    async fn replace_messages(&self, id: &SessionId, new_messages: Vec<ChatMessage>) {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            entry.messages = new_messages;
            entry.updated_at = Instant::now();
        }
    }

    async fn message_count(&self, id: &SessionId) -> usize {
        self.sessions.get(id).map(|s| s.messages.len()).unwrap_or(0)
    }

    async fn count(&self) -> usize {
        self.sessions.len()
    }

    async fn clear_user_sessions(&self, user_id: UserId) -> usize {
        let before = self.sessions.len();
        self.sessions
            .retain(|_id, session| session.user_id != Some(user_id));
        before - self.sessions.len()
    }

    async fn cleanup_stale(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.sessions
            .retain(|_id, session| session.updated_at > cutoff);
    }

    async fn get_summary(&self, id: &SessionId) -> Option<(String, usize)> {
        self.sessions.get(id).and_then(|s| {
            s.summary
                .as_ref()
                .map(|sum| (sum.clone(), s.summary_message_count))
        })
    }

    async fn set_summary(&self, id: &SessionId, summary: String, message_count: usize) {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            entry.summary = Some(summary);
            entry.summary_message_count = message_count;
            entry.updated_at = Instant::now();
        }
    }

    async fn attach(&self, id: &SessionId, attachments: Vec<SessionAttachment>) {
        let now = Instant::now();
        let mut entry = self.sessions.entry(*id).or_insert_with(|| Session {
            messages: Vec::new(),
            user_id: None,
            created_at: now,
            updated_at: now,
            summary: None,
            summary_message_count: 0,
            attachments: Vec::new(),
        });
        entry.attachments = attachments;
        entry.updated_at = now;
    }

    async fn get_attachments(&self, id: &SessionId) -> Vec<SessionAttachment> {
        self.sessions
            .get(id)
            .map(|s| s.attachments.clone())
            .unwrap_or_default()
    }

    async fn clear_attachments(&self, id: &SessionId) {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            entry.attachments.clear();
            entry.updated_at = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            images: vec![],
        }
    }

    #[tokio::test]
    async fn append_and_get_history() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        assert!(store.get_history(&sid).await.is_none());

        store
            .append(sid, msg("user", "hi"), msg("assistant", "hello"), None)
            .await;
        let history = store.get_history(&sid).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
    }

    #[tokio::test]
    async fn caps_at_max_history() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        // Add 30 turns = 60 messages, should be capped to 50
        for i in 0..30 {
            store
                .append(
                    sid,
                    msg("user", &format!("q{i}")),
                    msg("assistant", &format!("a{i}")),
                    None,
                )
                .await;
        }

        let history = store.get_history(&sid).await.unwrap();
        assert_eq!(history.len(), MAX_HISTORY);
    }

    #[tokio::test]
    async fn cleanup_removes_stale() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());
        store
            .append(sid, msg("user", "hi"), msg("assistant", "hey"), None)
            .await;

        store.cleanup_stale(Duration::ZERO).await;
        assert!(store.get_history(&sid).await.is_none());
    }

    #[tokio::test]
    async fn count_returns_session_count() {
        let store = InMemorySessionStore::new();
        assert_eq!(store.count().await, 0);

        let sid1 = SessionId(Uuid::new_v4());
        store
            .append(sid1, msg("user", "hi"), msg("assistant", "hey"), None)
            .await;
        assert_eq!(store.count().await, 1);

        let sid2 = SessionId(Uuid::new_v4());
        store
            .append(sid2, msg("user", "hello"), msg("assistant", "world"), None)
            .await;
        assert_eq!(store.count().await, 2);
    }

    #[tokio::test]
    async fn clear_user_sessions_removes_only_matching() {
        let store = InMemorySessionStore::new();
        let uid1 = UserId(Uuid::new_v4());
        let uid2 = UserId(Uuid::new_v4());

        let sid1 = SessionId(Uuid::new_v4());
        store
            .append(sid1, msg("user", "hi"), msg("assistant", "hey"), Some(uid1))
            .await;

        let sid2 = SessionId(Uuid::new_v4());
        store
            .append(
                sid2,
                msg("user", "hello"),
                msg("assistant", "world"),
                Some(uid2),
            )
            .await;

        let sid3 = SessionId(Uuid::new_v4());
        store
            .append(sid3, msg("user", "yo"), msg("assistant", "sup"), Some(uid1))
            .await;

        assert_eq!(store.count().await, 3);

        let cleared = store.clear_user_sessions(uid1).await;
        assert_eq!(cleared, 2);
        assert_eq!(store.count().await, 1);
        assert!(store.get_history(&sid1).await.is_none());
        assert!(store.get_history(&sid2).await.is_some());
        assert!(store.get_history(&sid3).await.is_none());
    }

    #[tokio::test]
    async fn cleanup_retains_fresh() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());
        store
            .append(sid, msg("user", "hi"), msg("assistant", "hey"), None)
            .await;

        store.cleanup_stale(Duration::from_secs(3600)).await;
        assert!(store.get_history(&sid).await.is_some());
    }

    fn attachment(name: &str, text: &str) -> SessionAttachment {
        SessionAttachment {
            name: name.to_string(),
            mime_type: "text/plain".to_string(),
            text: text.to_string(),
            size_bytes: text.len(),
            content_hash: "deadbeef".to_string(),
        }
    }

    #[tokio::test]
    async fn attach_and_get_attachments() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        // No attachments on an unknown session.
        assert!(store.get_attachments(&sid).await.is_empty());

        // attach() creates the session even before any message exists.
        store
            .attach(&sid, vec![attachment("report.pdf", "quarterly numbers")])
            .await;
        let got = store.get_attachments(&sid).await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "report.pdf");
        assert_eq!(got[0].text, "quarterly numbers");
    }

    #[tokio::test]
    async fn attach_replaces_prior_attachments() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        store.attach(&sid, vec![attachment("a.txt", "first")]).await;
        store
            .attach(
                &sid,
                vec![attachment("b.txt", "second"), attachment("c.txt", "third")],
            )
            .await;

        let got = store.get_attachments(&sid).await;
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "b.txt");
        assert_eq!(got[1].name, "c.txt");
    }

    #[tokio::test]
    async fn clear_attachments_empties_session() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        store.attach(&sid, vec![attachment("a.txt", "hello")]).await;
        store.clear_attachments(&sid).await;
        assert!(store.get_attachments(&sid).await.is_empty());
    }

    #[tokio::test]
    async fn attachments_survive_message_append() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        store
            .attach(&sid, vec![attachment("a.txt", "doc text")])
            .await;
        store
            .append(sid, msg("user", "hi"), msg("assistant", "hello"), None)
            .await;

        // History and attachments coexist on the same session.
        assert_eq!(store.get_history(&sid).await.unwrap().len(), 2);
        assert_eq!(store.get_attachments(&sid).await.len(), 1);
    }

    #[tokio::test]
    async fn get_set_summary() {
        let store = InMemorySessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        // No summary before session exists
        assert!(store.get_summary(&sid).await.is_none());

        // Create session
        store
            .append(sid, msg("user", "hi"), msg("assistant", "hey"), None)
            .await;

        // No summary yet
        assert!(store.get_summary(&sid).await.is_none());

        // Set summary
        store
            .set_summary(&sid, "User greeted the assistant.".into(), 2)
            .await;

        let (summary, count) = store.get_summary(&sid).await.unwrap();
        assert_eq!(summary, "User greeted the assistant.");
        assert_eq!(count, 2);

        // Update summary
        store.set_summary(&sid, "Updated summary.".into(), 10).await;
        let (summary, count) = store.get_summary(&sid).await.unwrap();
        assert_eq!(summary, "Updated summary.");
        assert_eq!(count, 10);
    }
}
