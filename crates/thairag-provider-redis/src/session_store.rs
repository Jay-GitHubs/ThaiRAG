use std::time::Duration;

use async_trait::async_trait;
use redis::AsyncCommands;
use thairag_core::traits::SessionStoreTrait;
use thairag_core::types::{ChatMessage, SessionAttachment, SessionId, UserId};

use crate::RedisConnection;

/// Redis-backed session store. Each session is stored as a JSON-serialized
/// list of messages under `session:{session_id}`. TTL is refreshed on each
/// access, replacing the need for periodic `cleanup_stale` calls.
pub struct RedisSessionStore {
    conn: RedisConnection,
    max_history: usize,
    ttl_secs: u64,
    /// Key prefix for user → session mapping.
    prefix: String,
}

/// Internal session data stored in Redis.
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionData {
    messages: Vec<ChatMessage>,
    user_id: Option<String>,
    /// Per-request document attachments active for this session.
    #[serde(default)]
    attachments: Vec<SessionAttachment>,
}

impl RedisSessionStore {
    pub fn new(conn: RedisConnection, max_history: usize, ttl_secs: u64) -> Self {
        Self {
            conn,
            max_history,
            ttl_secs,
            prefix: "session".into(),
        }
    }

    fn session_key(&self, id: &SessionId) -> String {
        format!("{}:{}", self.prefix, id.0)
    }

    fn user_sessions_key(&self, user_id: &UserId) -> String {
        format!("{}:user:{}", self.prefix, user_id.0)
    }
}

#[async_trait]
impl SessionStoreTrait for RedisSessionStore {
    async fn get_history(&self, session_id: &SessionId) -> Option<Vec<ChatMessage>> {
        let key = self.session_key(session_id);
        let mut conn = self.conn.manager();
        let data: Option<String> = conn.get(&key).await.ok()?;
        let data = data?;
        let session: SessionData = serde_json::from_str(&data).ok()?;

        // Refresh TTL on access
        let _: Result<(), _> = conn.expire::<_, ()>(&key, self.ttl_secs as i64).await;

        Some(session.messages)
    }

    async fn append(
        &self,
        session_id: SessionId,
        user_msg: ChatMessage,
        assistant_msg: ChatMessage,
        user_id: Option<UserId>,
    ) {
        let key = self.session_key(&session_id);
        let mut conn = self.conn.manager();

        // Get existing session or create new
        let mut session = match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(data)) => serde_json::from_str::<SessionData>(&data).unwrap_or(SessionData {
                messages: Vec::new(),
                user_id: user_id.map(|u| u.0.to_string()),
                attachments: Vec::new(),
            }),
            _ => SessionData {
                messages: Vec::new(),
                user_id: user_id.map(|u| u.0.to_string()),
                attachments: Vec::new(),
            },
        };

        session.messages.push(user_msg);
        session.messages.push(assistant_msg);

        // Trim to max history
        if session.messages.len() > self.max_history {
            let drain_count = session.messages.len() - self.max_history;
            session.messages.drain(..drain_count);
        }

        // Track user → session mapping for clear_user_sessions
        if let Some(uid) = user_id {
            let user_key = self.user_sessions_key(&uid);
            let _: Result<(), _> = conn
                .sadd::<_, _, ()>(&user_key, session_id.0.to_string())
                .await;
            let _: Result<(), _> = conn.expire::<_, ()>(&user_key, self.ttl_secs as i64).await;
        }

        if let Ok(json) = serde_json::to_string(&session) {
            let _: Result<(), _> = conn.set_ex::<_, _, ()>(&key, json, self.ttl_secs).await;
        }
    }

    async fn replace_messages(&self, session_id: &SessionId, new_messages: Vec<ChatMessage>) {
        let key = self.session_key(session_id);
        let mut conn = self.conn.manager();

        // Preserve user_id and attachments from existing session
        let (user_id, attachments) = match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(data)) => serde_json::from_str::<SessionData>(&data)
                .ok()
                .map(|s| (s.user_id, s.attachments))
                .unwrap_or((None, Vec::new())),
            _ => (None, Vec::new()),
        };

        let session = SessionData {
            messages: new_messages,
            user_id,
            attachments,
        };

        if let Ok(json) = serde_json::to_string(&session) {
            let _: Result<(), _> = conn.set_ex::<_, _, ()>(&key, json, self.ttl_secs).await;
        }
    }

    async fn message_count(&self, session_id: &SessionId) -> usize {
        let key = self.session_key(session_id);
        let mut conn = self.conn.manager();
        match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(data)) => serde_json::from_str::<SessionData>(&data)
                .map(|s| s.messages.len())
                .unwrap_or(0),
            _ => 0,
        }
    }

    async fn count(&self) -> usize {
        let mut conn = self.conn.manager();
        let pattern = format!("{}:*", self.prefix);
        // Use SCAN to count session keys (exclude user: keys)
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();
        keys.iter().filter(|k| !k.contains(":user:")).count()
    }

    async fn clear_user_sessions(&self, user_id: UserId) -> usize {
        let user_key = self.user_sessions_key(&user_id);
        let mut conn = self.conn.manager();

        // Get all session IDs for this user
        let session_ids: Vec<String> = conn.smembers(&user_key).await.unwrap_or_default();

        let count = session_ids.len();

        // Delete each session
        for sid in &session_ids {
            let key = format!("{}:{}", self.prefix, sid);
            let _: Result<(), _> = conn.del::<_, ()>(&key).await;
        }

        // Delete the user → sessions mapping
        let _: Result<(), _> = conn.del::<_, ()>(&user_key).await;

        count
    }

    async fn cleanup_stale(&self, _max_age: Duration) {
        // No-op for Redis: TTL handles expiration automatically.
    }

    async fn attach(&self, session_id: &SessionId, attachments: Vec<SessionAttachment>) {
        let key = self.session_key(session_id);
        let mut conn = self.conn.manager();

        // Load existing session (preserving messages + user_id) or start fresh.
        let mut session = match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(data)) => serde_json::from_str::<SessionData>(&data).unwrap_or(SessionData {
                messages: Vec::new(),
                user_id: None,
                attachments: Vec::new(),
            }),
            _ => SessionData {
                messages: Vec::new(),
                user_id: None,
                attachments: Vec::new(),
            },
        };

        session.attachments = attachments;

        if let Ok(json) = serde_json::to_string(&session) {
            let _: Result<(), _> = conn.set_ex::<_, _, ()>(&key, json, self.ttl_secs).await;
        }
    }

    async fn get_attachments(&self, session_id: &SessionId) -> Vec<SessionAttachment> {
        let key = self.session_key(session_id);
        let mut conn = self.conn.manager();
        match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(data)) => serde_json::from_str::<SessionData>(&data)
                .map(|s| s.attachments)
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    async fn clear_attachments(&self, session_id: &SessionId) {
        let key = self.session_key(session_id);
        let mut conn = self.conn.manager();
        if let Ok(Some(data)) = conn.get::<_, Option<String>>(&key).await
            && let Ok(mut session) = serde_json::from_str::<SessionData>(&data)
        {
            session.attachments.clear();
            if let Ok(json) = serde_json::to_string(&session) {
                let _: Result<(), _> = conn.set_ex::<_, _, ()>(&key, json, self.ttl_secs).await;
            }
        }
    }
}
