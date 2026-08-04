use redis::Client;
use redis::aio::ConnectionManager;

/// Shared Redis connection pool using `ConnectionManager` (auto-reconnecting).
#[derive(Clone)]
pub struct RedisConnection {
    manager: ConnectionManager,
}

impl RedisConnection {
    /// How long the initial connection may take before we give up. Without
    /// this bound, `ConnectionManager::new` retries an unreachable host with
    /// exponential backoff essentially forever — which silently hangs server
    /// boot when a tier preset selects Redis but the deployment has none
    /// (e.g. the lean registry stack). Bounded failure lets the callers'
    /// fall-back-to-memory arms actually run.
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    /// Create a new connection from a Redis URL (e.g. `redis://127.0.0.1:6379`).
    pub async fn new(url: &str) -> Result<Self, redis::RedisError> {
        let client = Client::open(url)?;
        let manager = tokio::time::timeout(Self::CONNECT_TIMEOUT, ConnectionManager::new(client))
            .await
            .map_err(|_elapsed| {
                redis::RedisError::from((
                    redis::ErrorKind::IoError,
                    "timed out connecting to Redis",
                ))
            })??;
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
