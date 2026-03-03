use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::extract::ConnectInfo;
use axum::http::{Request, Response, StatusCode};
use axum::body::Body;
use dashmap::DashMap;
use tower::{Layer, Service};

// ── Token Bucket ────────────────────────────────────────────────────

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// Per-IP token-bucket rate limiter.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<IpAddr, Bucket>>,
    rate: f64,      // tokens per second
    burst: f64,     // max tokens (bucket capacity)
}

impl RateLimiter {
    pub fn new(requests_per_second: u64, burst_size: u64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            rate: requests_per_second as f64,
            burst: burst_size as f64,
        }
    }

    /// Evict buckets that haven't been touched for longer than `max_age`.
    pub fn cleanup_stale(&self, max_age: Duration) {
        let cutoff = Instant::now() - max_age;
        self.buckets.retain(|_ip, bucket| bucket.last_refill > cutoff);
    }

    /// Try to consume one token for `ip`. Returns `Ok(())` if allowed,
    /// or `Err(retry_after_secs)` if rate-limited.
    fn try_acquire(&self, ip: IpAddr) -> Result<(), f64> {
        let now = Instant::now();
        let mut entry = self.buckets.entry(ip).or_insert_with(|| Bucket {
            tokens: self.burst,
            last_refill: now,
        });

        let bucket = entry.value_mut();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.rate).min(self.burst);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - bucket.tokens) / self.rate;
            Err(wait)
        }
    }
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
        let ip = extract_client_ip(&req);
        let mut inner = self.inner.clone();

        match self.limiter.try_acquire(ip) {
            Ok(()) => Box::pin(async move { inner.call(req).await }),
            Err(retry_after) => {
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
/// Priority: X-Forwarded-For header → ConnectInfo → fallback 0.0.0.0
fn extract_client_ip(req: &Request<Body>) -> IpAddr {
    // Check X-Forwarded-For first (first IP in the list)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(val) = forwarded.to_str() {
            if let Some(first) = val.split(',').next() {
                if let Ok(ip) = first.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    // Check ConnectInfo extension
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
}
