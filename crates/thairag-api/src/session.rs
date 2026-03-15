use std::time::{Duration, Instant};

use dashmap::DashMap;
use thairag_core::types::{ChatMessage, SessionId};

const MAX_HISTORY: usize = 50;

struct Session {
    messages: Vec<ChatMessage>,
    #[allow(dead_code)]
    created_at: Instant,
    updated_at: Instant,
}

#[derive(Clone)]
pub struct SessionStore {
    sessions: std::sync::Arc<DashMap<SessionId, Session>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: std::sync::Arc::new(DashMap::new()),
        }
    }

    pub fn get_history(&self, id: &SessionId) -> Option<Vec<ChatMessage>> {
        self.sessions.get(id).map(|s| s.messages.clone())
    }

    pub fn append(&self, id: SessionId, user_msg: ChatMessage, assistant_msg: ChatMessage) {
        let now = Instant::now();
        let mut entry = self.sessions.entry(id).or_insert_with(|| Session {
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
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

    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    pub fn cleanup_stale(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.sessions
            .retain(|_id, session| session.updated_at > cutoff);
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
        }
    }

    #[test]
    fn append_and_get_history() {
        let store = SessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        assert!(store.get_history(&sid).is_none());

        store.append(sid, msg("user", "hi"), msg("assistant", "hello"));
        let history = store.get_history(&sid).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
    }

    #[test]
    fn caps_at_max_history() {
        let store = SessionStore::new();
        let sid = SessionId(Uuid::new_v4());

        // Add 30 turns = 60 messages, should be capped to 50
        for i in 0..30 {
            store.append(
                sid,
                msg("user", &format!("q{i}")),
                msg("assistant", &format!("a{i}")),
            );
        }

        let history = store.get_history(&sid).unwrap();
        assert_eq!(history.len(), MAX_HISTORY);
    }

    #[test]
    fn cleanup_removes_stale() {
        let store = SessionStore::new();
        let sid = SessionId(Uuid::new_v4());
        store.append(sid, msg("user", "hi"), msg("assistant", "hey"));

        store.cleanup_stale(Duration::ZERO);
        assert!(store.get_history(&sid).is_none());
    }

    #[test]
    fn count_returns_session_count() {
        let store = SessionStore::new();
        assert_eq!(store.count(), 0);

        let sid1 = SessionId(Uuid::new_v4());
        store.append(sid1, msg("user", "hi"), msg("assistant", "hey"));
        assert_eq!(store.count(), 1);

        let sid2 = SessionId(Uuid::new_v4());
        store.append(sid2, msg("user", "hello"), msg("assistant", "world"));
        assert_eq!(store.count(), 2);
    }

    #[test]
    fn cleanup_retains_fresh() {
        let store = SessionStore::new();
        let sid = SessionId(Uuid::new_v4());
        store.append(sid, msg("user", "hi"), msg("assistant", "hey"));

        store.cleanup_stale(Duration::from_secs(3600));
        assert!(store.get_history(&sid).is_some());
    }
}
