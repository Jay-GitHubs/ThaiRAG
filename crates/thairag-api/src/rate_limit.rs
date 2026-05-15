use std::collections::VecDeque;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, Response, StatusCode};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use tower::{Layer, Service};

// ── Blocked Event Ring Buffer ──────────────────────────────────────

const MAX_BLOCKED_EVENTS: usize = 100;

#[derive(Clone, Serialize)]
pub struct BlockedEvent {
    pub timestamp: DateTime<Utc>,
    pub source: String,      // IP address or user ID
    pub source_type: String, // "ip" or "user"
    pub endpoint: String,
    pub reason: String,
}

/// Thread-safe ring buffer for recently blocked requests.
#[derive(Clone)]
pub struct BlockedEventLog {
    events: Arc<Mutex<VecDeque<BlockedEvent>>>,
    total_blocked: Arc<AtomicU64>,
}

impl BlockedEventLog {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_BLOCKED_EVENTS))),
            total_blocked: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record(&self, event: BlockedEvent) {
        self.total_blocked.fetch_add(1, Ordering::Relaxed);
        let mut events = self.events.lock().unwrap();
        if events.len() >= MAX_BLOCKED_EVENTS {
            events.pop_front();
        }
        events.push_back(event);
    }

    pub fn recent(&self) -> Vec<BlockedEvent> {
        self.events.lock().unwrap().iter().rev().cloned().collect()
    }

    pub fn total_blocked(&self) -> u64 {
        self.total_blocked.load(Ordering::Relaxed)
    }
}

impl Default for BlockedEventLog {
    fn default() -> Self {
        Self::new()
    }
}

// ── Token Bucket ────────────────────────────────────────────────────

struct Bucket {
    tokens: f64,
    last_refill: Instant,
    request_count: u64,
}

/// Per-IP token-bucket rate limiter.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<IpAddr, Bucket>>,
    rate: f64,  // tokens per second
    burst: f64, // max tokens (bucket capacity)
    trust_proxy: bool,
    blocked_log: BlockedEventLog,
}

impl RateLimiter {
    pub fn new(requests_per_second: u64, burst_size: u64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            rate: requests_per_second as f64,
            burst: burst_size as f64,
            trust_proxy: false,
            blocked_log: BlockedEventLog::new(),
        }
    }

    pub fn with_trust_proxy(mut self, trust: bool) -> Self {
        self.trust_proxy = trust;
        self
    }

    pub fn blocked_log(&self) -> &BlockedEventLog {
        &self.blocked_log
    }

    /// Evict buckets that haven't been touched for longer than `max_age`.
    pub fn cleanup_stale(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.buckets
            .retain(|_ip, bucket| bucket.last_refill > cutoff);
    }

    /// Try to consume one token for `ip`. Returns `Ok(())` if allowed,
    /// or `Err(retry_after_secs)` if rate-limited.
    fn try_acquire(&self, ip: IpAddr) -> Result<(), f64> {
        let now = Instant::now();
        let mut entry = self.buckets.entry(ip).or_insert_with(|| Bucket {
            tokens: self.burst,
            last_refill: now,
            request_count: 0,
        });

        let bucket = entry.value_mut();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.rate).min(self.burst);
        bucket.last_refill = now;
        bucket.request_count += 1;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - bucket.tokens) / self.rate;
            Err(wait)
        }
    }

    /// Get stats for all active IP buckets (top N by request count).
    pub fn ip_stats(&self, top_n: usize) -> Vec<IpBucketStats> {
        let now = Instant::now();
        let mut stats: Vec<IpBucketStats> = self
            .buckets
            .iter()
            .map(|entry| {
                let ip = *entry.key();
                let bucket = entry.value();
                let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
                let current_tokens = (bucket.tokens + elapsed * self.rate).min(self.burst);
                IpBucketStats {
                    ip: ip.to_string(),
                    request_count: bucket.request_count,
                    tokens_remaining: current_tokens,
                    last_seen_secs_ago: elapsed,
                }
            })
            .collect();
        stats.sort_by_key(|s| std::cmp::Reverse(s.request_count));
        stats.truncate(top_n);
        stats
    }

    /// Number of active IP limiters.
    pub fn active_count(&self) -> usize {
        self.buckets.len()
    }
}

#[derive(Serialize)]
pub struct IpBucketStats {
    pub ip: String,
    pub request_count: u64,
    pub tokens_remaining: f64,
    pub last_seen_secs_ago: f64,
}

// ── Per-user token-bucket rate limiter ──────────────────────────

struct UserBucket {
    tokens: f64,
    last_refill: Instant,
    request_count: u64,
}

/// Token-bucket rate limiter keyed by user ID (string).
/// Applied after authentication to limit per-user request rates.
#[derive(Clone)]
pub struct UserRateLimiter {
    buckets: Arc<DashMap<String, UserBucket>>,
    rate: f64,
    burst: f64,
    blocked_log: BlockedEventLog,
}

impl UserRateLimiter {
    pub fn new(requests_per_second: u64, burst_size: u64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            rate: requests_per_second as f64,
            burst: burst_size as f64,
            blocked_log: BlockedEventLog::new(),
        }
    }

    pub fn blocked_log(&self) -> &BlockedEventLog {
        &self.blocked_log
    }

    /// Try to consume one token for `user_id`. Returns `Ok(())` if allowed,
    /// or `Err(retry_after_secs)` if rate-limited.
    pub fn try_acquire(&self, user_id: &str) -> Result<(), f64> {
        let now = Instant::now();
        let mut entry = self
            .buckets
            .entry(user_id.to_string())
            .or_insert_with(|| UserBucket {
                tokens: self.burst,
                last_refill: now,
                request_count: 0,
            });

        let bucket = entry.value_mut();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.rate).min(self.burst);
        bucket.last_refill = now;
        bucket.request_count += 1;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - bucket.tokens) / self.rate;
            // Record blocked event
            self.blocked_log.record(BlockedEvent {
                timestamp: Utc::now(),
                source: user_id.to_string(),
                source_type: "user".to_string(),
                endpoint: String::new(), // filled by caller if needed
                reason: format!("User rate limit exceeded, retry after {wait:.1}s"),
            });
            Err(wait)
        }
    }

    /// Evict buckets that haven't been touched for longer than `max_age`.
    pub fn cleanup_stale(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.buckets.retain(|_, bucket| bucket.last_refill > cutoff);
    }

    /// Get stats for all active user buckets (top N by request count).
    pub fn user_stats(&self, top_n: usize) -> Vec<UserBucketStats> {
        let now = Instant::now();
        let mut stats: Vec<UserBucketStats> = self
            .buckets
            .iter()
            .map(|entry| {
                let user_id = entry.key().clone();
                let bucket = entry.value();
                let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
                let current_tokens = (bucket.tokens + elapsed * self.rate).min(self.burst);
                UserBucketStats {
                    user_id,
                    request_count: bucket.request_count,
                    tokens_remaining: current_tokens,
                    last_seen_secs_ago: elapsed,
                }
            })
            .collect();
        stats.sort_by_key(|s| std::cmp::Reverse(s.request_count));
        stats.truncate(top_n);
        stats
    }

    /// Number of active user limiters.
    pub fn active_count(&self) -> usize {
        self.buckets.len()
    }
}

#[derive(Serialize)]
pub struct UserBucketStats {
    pub user_id: String,
    pub request_count: u64,
    pub tokens_remaining: f64,
    pub last_seen_secs_ago: f64,
}

// ── Layer ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: RateLimiter,
}

impl RateLimitLayer {
    pub fn new(limiter: RateLimiter) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

// ── Service ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: RateLimiter,
}

impl<S> Service<Request<Body>> for RateLimitService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let ip = extract_client_ip(&req, self.limiter.trust_proxy);
        let endpoint = req.uri().path().to_string();
        let mut inner = self.inner.clone();

        match self.limiter.try_acquire(ip) {
            Ok(()) => Box::pin(async move { inner.call(req).await }),
            Err(retry_after) => {
                // Record blocked event for IP rate limiting
                self.limiter.blocked_log.record(BlockedEvent {
                    timestamp: Utc::now(),
                    source: ip.to_string(),
                    source_type: "ip".to_string(),
                    endpoint,
                    reason: format!(
                        "IP rate limit exceeded, retry after {:.0}s",
                        retry_after.ceil()
                    ),
                });

                let retry_secs = retry_after.ceil() as u64;
                Box::pin(async move {
                    let resp = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header("retry-after", retry_secs.to_string())
                        .body(Body::from(
                            serde_json::json!({
                                "error": {
                                    "message": "Rate limit exceeded",
                                    "type": "rate_limit_error",
                                    "retry_after": retry_secs
                                }
                            })
                            .to_string(),
                        ))
                        .unwrap();
                    Ok(resp)
                })
            }
        }
    }
}

// ── IP Extraction ───────────────────────────────────────────────────

/// Extract client IP from the request.
///
/// When `trust_proxy` is false (default): use ConnectInfo (real TCP peer) only.
/// This prevents attackers from spoofing X-Forwarded-For to bypass rate limiting.
///
/// When `trust_proxy` is true: use X-Forwarded-For (last entry before proxy)
/// then fall back to ConnectInfo. Only enable when running behind a trusted
/// reverse proxy that sets this header.
fn extract_client_ip(req: &Request<Body>, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        // When behind a trusted proxy, use X-Forwarded-For.
        // Use the *first* IP (original client) — the proxy appends its own.
        if let Some(forwarded) = req.headers().get("x-forwarded-for")
            && let Ok(val) = forwarded.to_str()
            && let Some(first) = val.split(',').next()
            && let Ok(ip) = first.trim().parse::<IpAddr>()
        {
            return ip;
        }
    }

    // Use the actual TCP peer address
    if let Some(connect_info) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return connect_info.0.ip();
    }

    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_removes_stale_buckets() {
        let limiter = RateLimiter::new(10, 10);
        let ip1: IpAddr = "1.2.3.4".parse().unwrap();
        let ip2: IpAddr = "5.6.7.8".parse().unwrap();

        // Acquire tokens so entries exist
        limiter.try_acquire(ip1).unwrap();
        limiter.try_acquire(ip2).unwrap();
        assert_eq!(limiter.buckets.len(), 2);

        // max_age=0 means everything is stale
        limiter.cleanup_stale(Duration::ZERO);
        assert_eq!(limiter.buckets.len(), 0);
    }

    #[test]
    fn cleanup_retains_fresh_buckets() {
        let limiter = RateLimiter::new(10, 10);
        let ip1: IpAddr = "1.2.3.4".parse().unwrap();
        let ip2: IpAddr = "5.6.7.8".parse().unwrap();

        limiter.try_acquire(ip1).unwrap();
        limiter.try_acquire(ip2).unwrap();
        assert_eq!(limiter.buckets.len(), 2);

        // 1 hour is plenty — entries were just created
        limiter.cleanup_stale(Duration::from_secs(3600));
        assert_eq!(limiter.buckets.len(), 2);
    }

    #[test]
    fn ip_stats_returns_sorted() {
        let limiter = RateLimiter::new(100, 100);
        let ip1: IpAddr = "1.2.3.4".parse().unwrap();
        let ip2: IpAddr = "5.6.7.8".parse().unwrap();

        // ip1 gets 1 request, ip2 gets 3
        limiter.try_acquire(ip1).unwrap();
        limiter.try_acquire(ip2).unwrap();
        limiter.try_acquire(ip2).unwrap();
        limiter.try_acquire(ip2).unwrap();

        let stats = limiter.ip_stats(20);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].ip, "5.6.7.8");
        assert_eq!(stats[0].request_count, 3);
        assert_eq!(stats[1].ip, "1.2.3.4");
        assert_eq!(stats[1].request_count, 1);
    }

    #[test]
    fn blocked_event_log_ring_buffer() {
        let log = BlockedEventLog::new();
        for i in 0..150 {
            log.record(BlockedEvent {
                timestamp: Utc::now(),
                source: format!("user_{i}"),
                source_type: "user".into(),
                endpoint: "/test".into(),
                reason: "test".into(),
            });
        }
        assert_eq!(log.total_blocked(), 150);
        assert_eq!(log.recent().len(), MAX_BLOCKED_EVENTS);
        // Most recent should be first
        assert_eq!(log.recent()[0].source, "user_149");
    }

    #[test]
    fn user_stats_returns_sorted() {
        let limiter = UserRateLimiter::new(100, 100);
        limiter.try_acquire("alice").unwrap();
        limiter.try_acquire("bob").unwrap();
        limiter.try_acquire("bob").unwrap();

        let stats = limiter.user_stats(20);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].user_id, "bob");
        assert_eq!(stats[0].request_count, 2);
    }
}
