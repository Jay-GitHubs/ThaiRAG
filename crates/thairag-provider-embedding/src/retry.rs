//! Retry-with-backoff for transient upstream failures.
//!
//! OpenAI-compatible gateways return intermittent `502`/`503`/`504` while a
//! backend restarts, and occasionally drop the connection. A `502` on an
//! embedding call would otherwise fail the whole request (e.g. a chat query
//! that can't be vectorized → empty retrieval). A short exponential backoff
//! absorbs these blips transparently.

use std::time::Duration;

/// Number of retries after the initial attempt (total tries = `1 + MAX_RETRIES`).
pub const MAX_RETRIES: u32 = 3;
/// Base backoff; attempt `n` (1-based) waits `BASE_DELAY_MS << (n-1)`.
const BASE_DELAY_MS: u64 = 400;

/// Whether an HTTP status is a transient upstream failure worth retrying.
pub fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

/// Whether a transport error (no HTTP response) is transient and retryable.
pub fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

/// Backoff delay before retry attempt `attempt` (1-based).
fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_millis(BASE_DELAY_MS << (attempt - 1))
}

/// Send a request, retrying transient transport errors and retryable HTTP
/// statuses with exponential backoff. `build` must produce an equivalent
/// request on each call (invoked once per attempt). The returned response may
/// still carry a non-success status the caller should inspect — only
/// *retryable* statuses are retried.
pub async fn send_with_retry(
    build: impl Fn() -> reqwest::RequestBuilder,
    label: &str,
) -> reqwest::Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        match build().send().await {
            Ok(resp) => {
                if attempt < MAX_RETRIES && is_retryable_status(resp.status()) {
                    let status = resp.status();
                    attempt += 1;
                    let delay = backoff_delay(attempt);
                    tracing::warn!(
                        label,
                        %status,
                        attempt,
                        delay_ms = delay.as_millis() as u64,
                        "retryable upstream status; backing off before retry"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                if attempt < MAX_RETRIES && is_retryable_error(&e) {
                    attempt += 1;
                    let delay = backoff_delay(attempt);
                    tracing::warn!(
                        label,
                        error = %e,
                        attempt,
                        delay_ms = delay.as_millis() as u64,
                        "retryable transport error; backing off before retry"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_status_classification() {
        for code in [408, 425, 429, 500, 502, 503, 504] {
            assert!(is_retryable_status(
                reqwest::StatusCode::from_u16(code).unwrap()
            ));
        }
        for code in [200, 400, 401, 404, 422] {
            assert!(!is_retryable_status(
                reqwest::StatusCode::from_u16(code).unwrap()
            ));
        }
    }

    #[test]
    fn backoff_is_exponential() {
        assert_eq!(backoff_delay(1), Duration::from_millis(400));
        assert_eq!(backoff_delay(2), Duration::from_millis(800));
        assert_eq!(backoff_delay(3), Duration::from_millis(1600));
    }
}
