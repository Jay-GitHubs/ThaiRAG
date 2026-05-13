use async_trait::async_trait;
use redis::AsyncCommands;
use sha2::{Digest, Sha256};
use thairag_core::traits::EmbeddingCache;

use crate::RedisConnection;

/// Redis-backed embedding cache. Embeddings are stored as raw f32 bytes
/// under `emb:{sha256(text)}` with TTL-based expiration.
pub struct RedisEmbeddingCache {
    conn: RedisConnection,
    ttl_secs: u64,
    prefix: String,
}

impl RedisEmbeddingCache {
    pub fn new(conn: RedisConnection, ttl_secs: u64) -> Self {
        Self {
            conn,
            ttl_secs,
            prefix: "emb".into(),
        }
    }

    fn cache_key(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let hash = hex::encode(hasher.finalize());
        format!("{}:{}", self.prefix, hash)
    }

    /// Serialize a Vec<f32> to raw bytes (little-endian).
    fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
        embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    /// Deserialize raw bytes back to Vec<f32>.
    fn decode_embedding(data: &[u8]) -> Option<Vec<f32>> {
        if !data.len().is_multiple_of(4) {
            return None;
        }
        Some(
            data.chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect(),
        )
    }
}

#[async_trait]
impl EmbeddingCache for RedisEmbeddingCache {
    async fn get(&self, text: &str) -> Option<Vec<f32>> {
        let key = self.cache_key(text);
        let mut conn = self.conn.manager();
        let data: Option<Vec<u8>> = conn.get(&key).await.ok()?;
        data.and_then(|d| Self::decode_embedding(&d))
    }

    async fn get_many(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        if texts.is_empty() {
            return vec![];
        }

        let keys: Vec<String> = texts.iter().map(|t| self.cache_key(t)).collect();
        let mut conn = self.conn.manager();

        // Use MGET for batch retrieval
        let results: Vec<Option<Vec<u8>>> =
            match redis::cmd("MGET").arg(&keys).query_async(&mut conn).await {
                Ok(r) => r,
                Err(_) => return vec![None; texts.len()],
            };

        results
            .into_iter()
            .map(|opt| opt.and_then(|d| Self::decode_embedding(&d)))
            .collect()
    }

    async fn put(&self, text: &str, embedding: Vec<f32>) {
        let key = self.cache_key(text);
        let data = Self::encode_embedding(&embedding);
        let mut conn = self.conn.manager();
        let _: Result<(), _> = conn.set_ex::<_, _, ()>(&key, data, self.ttl_secs).await;
    }

    async fn put_many(&self, pairs: Vec<(String, Vec<f32>)>) {
        if pairs.is_empty() {
            return;
        }

        let mut conn = self.conn.manager();

        // Use pipeline for batch write
        let mut pipe = redis::pipe();
        for (text, embedding) in &pairs {
            let key = self.cache_key(text);
            let data = Self::encode_embedding(embedding);
            pipe.cmd("SETEX")
                .arg(&key)
                .arg(self.ttl_secs as i64)
                .arg(data)
                .ignore();
        }

        let _: Result<(), _> = pipe.query_async(&mut conn).await;
    }

    async fn len(&self) -> usize {
        let mut conn = self.conn.manager();
        let pattern = format!("{}:*", self.prefix);
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();
        keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let original = vec![1.0f32, -2.5, std::f32::consts::PI, 0.0, f32::MAX];
        let encoded = RedisEmbeddingCache::encode_embedding(&original);
        let decoded = RedisEmbeddingCache::decode_embedding(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn decode_invalid_length_returns_none() {
        assert!(RedisEmbeddingCache::decode_embedding(&[1, 2, 3]).is_none());
    }
}
