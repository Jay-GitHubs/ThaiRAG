use redis::Client;
use redis::aio::ConnectionManager;

/// Shared Redis connection pool using `ConnectionManager` (auto-reconnecting).
#[derive(Clone)]
pub struct RedisConnection {
    manager: ConnectionManager,
}

impl RedisConnection {
    /// Create a new connection from a Redis URL (e.g. `redis://127.0.0.1:6379`).
    pub async fn new(url: &str) -> Result<Self, redis::RedisError> {
        let client = Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;
        tracing::info!(url = %url, "Redis connection established");
        Ok(Self { manager })
    }

    /// Get a clone of the connection manager (cheaply cloneable, auto-reconnects).
    pub fn manager(&self) -> ConnectionManager {
        self.manager.clone()
    }

    /// Ping the Redis server to check connectivity.
    pub async fn ping(&self) -> Result<(), redis::RedisError> {
        let mut conn = self.manager.clone();
        redis::cmd("PING").query_async::<String>(&mut conn).await?;
        Ok(())
    }
}
