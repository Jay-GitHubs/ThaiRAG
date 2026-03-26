//! Integration tests for Redis-backed session store and embedding cache.
//!
//! These tests require a running Redis instance at `REDIS_TEST_URL`
//! (default: `redis://127.0.0.1:6379`). Tests are skipped automatically
//! if Redis is not available.

use std::time::Duration;

use thairag_core::traits::{EmbeddingCache, SessionStoreTrait};
use thairag_core::types::{ChatMessage, SessionId, UserId};
use thairag_provider_redis::{RedisConnection, RedisEmbeddingCache, RedisSessionStore};
use uuid::Uuid;

/// Try connecting to Redis; return None (skip) if unavailable.
async fn try_connect() -> Option<RedisConnection> {
    let url = std::env::var("REDIS_TEST_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let conn = RedisConnection::new(&url).await.ok()?;
    conn.ping().await.ok()?;
    Some(conn)
}

macro_rules! skip_if_no_redis {
    () => {
        match try_connect().await {
            Some(conn) => conn,
            None => {
                eprintln!("Skipping: Redis not available");
                return;
            }
        }
    };
}

// ── Session Store Tests ──────────────────────────────────────────────

#[tokio::test]
async fn session_append_and_get_history() {
    let conn = skip_if_no_redis!();
    let store = RedisSessionStore::new(conn, 50, 300);
    let sid = SessionId(Uuid::new_v4());

    let user_msg = ChatMessage {
        role: "user".into(),
        content: "hello".into(),
    };
    let assist_msg = ChatMessage {
        role: "assistant".into(),
        content: "hi there".into(),
    };

    store
        .append(sid, user_msg.clone(), assist_msg.clone(), None)
        .await;

    let history = store.get_history(&sid).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, "user");
    assert_eq!(history[1].role, "assistant");
}

#[tokio::test]
async fn session_caps_at_max_history() {
    let conn = skip_if_no_redis!();
    let store = RedisSessionStore::new(conn, 4, 300);
    let sid = SessionId(Uuid::new_v4());

    for i in 0..5 {
        store
            .append(
                sid,
                ChatMessage {
                    role: "user".into(),
                    content: format!("msg {i}"),
                },
                ChatMessage {
                    role: "assistant".into(),
                    content: format!("reply {i}"),
                },
                None,
            )
            .await;
    }

    let history = store.get_history(&sid).await.unwrap();
    assert_eq!(history.len(), 4); // capped at max_history
}

#[tokio::test]
async fn session_replace_messages() {
    let conn = skip_if_no_redis!();
    let store = RedisSessionStore::new(conn, 50, 300);
    let sid = SessionId(Uuid::new_v4());

    store
        .append(
            sid,
            ChatMessage {
                role: "user".into(),
                content: "old".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "old reply".into(),
            },
            None,
        )
        .await;

    let new_messages = vec![ChatMessage {
        role: "system".into(),
        content: "replaced".into(),
    }];
    store.replace_messages(&sid, new_messages).await;

    let history = store.get_history(&sid).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "replaced");
}

#[tokio::test]
async fn session_clear_user_sessions() {
    let conn = skip_if_no_redis!();
    let store = RedisSessionStore::new(conn, 50, 300);

    let uid = UserId(Uuid::new_v4());
    let sid1 = SessionId(Uuid::new_v4());
    let sid2 = SessionId(Uuid::new_v4());

    let msg = ChatMessage {
        role: "user".into(),
        content: "test".into(),
    };
    let reply = ChatMessage {
        role: "assistant".into(),
        content: "ok".into(),
    };

    store
        .append(sid1, msg.clone(), reply.clone(), Some(uid))
        .await;
    store
        .append(sid2, msg.clone(), reply.clone(), Some(uid))
        .await;

    let cleared = store.clear_user_sessions(uid).await;
    assert_eq!(cleared, 2);

    assert!(store.get_history(&sid1).await.is_none());
    assert!(store.get_history(&sid2).await.is_none());
}

#[tokio::test]
async fn session_message_count() {
    let conn = skip_if_no_redis!();
    let store = RedisSessionStore::new(conn, 50, 300);
    let sid = SessionId(Uuid::new_v4());

    assert_eq!(store.message_count(&sid).await, 0);

    store
        .append(
            sid,
            ChatMessage {
                role: "user".into(),
                content: "a".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "b".into(),
            },
            None,
        )
        .await;

    assert_eq!(store.message_count(&sid).await, 2);
}

#[tokio::test]
async fn session_cleanup_is_noop() {
    let conn = skip_if_no_redis!();
    let store = RedisSessionStore::new(conn, 50, 300);
    // Just verify it doesn't panic
    store.cleanup_stale(Duration::from_secs(60)).await;
}

// ── Embedding Cache Tests ────────────────────────────────────────────

#[tokio::test]
async fn cache_put_and_get() {
    let conn = skip_if_no_redis!();
    let cache = RedisEmbeddingCache::new(conn, 300);

    let text = format!("test-embedding-{}", Uuid::new_v4());
    let embedding = vec![1.0f32, 2.0, 3.0, 4.0];

    cache.put(&text, embedding.clone()).await;
    let got = cache.get(&text).await.unwrap();
    assert_eq!(got, embedding);
}

#[tokio::test]
async fn cache_miss_returns_none() {
    let conn = skip_if_no_redis!();
    let cache = RedisEmbeddingCache::new(conn, 300);

    let result = cache.get(&format!("nonexistent-{}", Uuid::new_v4())).await;
    assert!(result.is_none());
}

#[tokio::test]
async fn cache_put_many_get_many() {
    let conn = skip_if_no_redis!();
    let cache = RedisEmbeddingCache::new(conn, 300);

    let prefix = Uuid::new_v4().to_string();
    let texts: Vec<String> = (0..3).map(|i| format!("{prefix}-text-{i}")).collect();
    let pairs: Vec<(String, Vec<f32>)> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| (t.clone(), vec![i as f32; 4]))
        .collect();

    cache.put_many(pairs).await;

    // get_many with a mix of hits and misses
    let query = vec![
        texts[0].clone(),
        format!("{prefix}-missing"),
        texts[2].clone(),
    ];
    let results = cache.get_many(&query).await;

    assert_eq!(results.len(), 3);
    assert!(results[0].is_some());
    assert!(results[1].is_none());
    assert!(results[2].is_some());
    assert_eq!(results[0].as_ref().unwrap(), &vec![0.0f32; 4]);
    assert_eq!(results[2].as_ref().unwrap(), &vec![2.0f32; 4]);
}
