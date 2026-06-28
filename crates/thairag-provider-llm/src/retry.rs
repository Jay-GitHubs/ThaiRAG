//! Retry-with-backoff for transient upstream failures.
//!
//! OpenAI-compatible gateways (load balancers in front of model workers) return
//! intermittent `502`/`503`/`504` while a backend restarts, and occasionally
//! drop the connection. Without retry, a single flap fails a whole chat request
//! (a `502` on the query-embedding call → empty retrieval → no answer). A short
//! exponential backoff absorbs these blips transparently.

use std::time::Duration;

/// Number of retries after the initial attempt (total tries = `1 + MAX_RETRIES`).
/// Sized to ride out a flaky gateway that 5xx-flaps for several seconds while a
/// backend worker recycles — too few retries let a brief flap fail a whole chat
/// (which fans out to ~8 gateway calls; one unrecovered 5xx fails the request).
pub const MAX_RETRIES: u32 = 5;
/// Base backoff; attempt `n` (1-based) waits `min(BASE_DELAY_MS << (n-1), MAX_DELAY_MS)`.
const BASE_DELAY_MS: u64 = 400;
/// Cap on a single backoff so the later attempts don't add excessive latency.
const MAX_DELAY_MS: u64 = 3000;

/// Whether an HTTP status is a transient upstream failure worth retrying.
/// Mirrors the common client policy (timeouts, rate-limit, and gateway 5xx).
pub fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

/// Whether a transport error (no HTTP response) is transient and retryable.
pub fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

/// Backoff delay before retry attempt `attempt` (1-based), capped at `MAX_DELAY_MS`.
fn backoff_delay(attempt: u32) -> Duration {
    let raw = BASE_DELAY_MS.checked_shl(attempt - 1).unwrap_or(u64::MAX);
    Duration::from_millis(raw.min(MAX_DELAY_MS))
}

/// Send a request, retrying transient transport errors and retryable HTTP
/// statuses with exponential backoff. `build` must produce an equivalent
/// request on each call (it is invoked once per attempt). The returned response
/// may still carry a non-success status the caller should inspect — only
/// *retryable* statuses are retried; a `400`/`401`/etc. is returned as-is.
///
/// For streaming, call this for the initial request only: once bytes are being
/// consumed a retry would duplicate partial output, so mid-stream failures are
/// not retried here.
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
            assert!(
                is_retryable_status(reqwest::StatusCode::from_u16(code).unwrap()),
                "{code} should be retryable"
            );
        }
        for code in [200, 400, 401, 403, 404, 422] {
            assert!(
                !is_retryable_status(reqwest::StatusCode::from_u16(code).unwrap()),
                "{code} should NOT be retryable"
            );
        }
    }

    #[test]
    fn backoff_is_exponential_then_capped() {
        assert_eq!(backoff_delay(1), Duration::from_millis(400));
        assert_eq!(backoff_delay(2), Duration::from_millis(800));
        assert_eq!(backoff_delay(3), Duration::from_millis(1600));
        // Later attempts are capped so the tail doesn't add huge latency.
        assert_eq!(backoff_delay(4), Duration::from_millis(3000));
        assert_eq!(backoff_delay(5), Duration::from_millis(3000));
    }
}
