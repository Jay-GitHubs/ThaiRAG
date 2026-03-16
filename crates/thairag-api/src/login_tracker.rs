use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;

/// Tracks failed login attempts per email for brute-force protection.
#[derive(Clone)]
pub struct LoginTracker {
    inner: Arc<LoginTrackerInner>,
}

struct LoginTrackerInner {
    attempts: DashMap<String, LoginAttemptState>,
    max_attempts: u32,
    lockout_secs: u64,
}

#[derive(Clone)]
struct LoginAttemptState {
    count: u32,
    first_attempt: Instant,
    locked_until: Option<Instant>,
}

impl LoginTracker {
    pub fn new(max_attempts: u32, lockout_secs: u64) -> Self {
        Self {
            inner: Arc::new(LoginTrackerInner {
                attempts: DashMap::new(),
                max_attempts,
                lockout_secs,
            }),
        }
    }

    /// Returns `true` if the email is currently locked out.
    pub fn is_locked(&self, email: &str) -> bool {
        let key = email.to_lowercase();
        if let Some(state) = self.inner.attempts.get(&key)
            && let Some(locked_until) = state.locked_until
            && Instant::now() < locked_until
        {
            return true;
        }
        false
    }

    /// Record a failed login attempt. Returns `true` if the account is now locked.
    pub fn record_failure(&self, email: &str) -> bool {
        let key = email.to_lowercase();
        let mut entry = self
            .inner
            .attempts
            .entry(key)
            .or_insert_with(|| LoginAttemptState {
                count: 0,
                first_attempt: Instant::now(),
                locked_until: None,
            });

        // Reset if the lockout window has expired
        if let Some(locked_until) = entry.locked_until
            && Instant::now() >= locked_until
        {
            entry.count = 0;
            entry.locked_until = None;
            entry.first_attempt = Instant::now();
        }

        entry.count += 1;

        if entry.count >= self.inner.max_attempts {
            entry.locked_until =
                Some(Instant::now() + std::time::Duration::from_secs(self.inner.lockout_secs));
            tracing::warn!(
                email = %entry.key(),
                attempts = entry.count,
                lockout_secs = self.inner.lockout_secs,
                "Account locked due to too many failed login attempts"
            );
            return true;
        }
        false
    }

    /// Clear tracking for an email on successful login.
    pub fn record_success(&self, email: &str) {
        self.inner.attempts.remove(&email.to_lowercase());
    }

    /// Remaining lockout seconds (for Retry-After header).
    pub fn lockout_remaining_secs(&self, email: &str) -> u64 {
        let key = email.to_lowercase();
        if let Some(state) = self.inner.attempts.get(&key)
            && let Some(locked_until) = state.locked_until
        {
            let remaining = locked_until.saturating_duration_since(Instant::now());
            return remaining.as_secs();
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_failed_attempts_and_locks() {
        let tracker = LoginTracker::new(3, 60);
        assert!(!tracker.is_locked("user@test.com"));

        assert!(!tracker.record_failure("user@test.com"));
        assert!(!tracker.record_failure("user@test.com"));
        assert!(tracker.record_failure("user@test.com")); // 3rd attempt → locked

        assert!(tracker.is_locked("user@test.com"));
    }

    #[test]
    fn success_clears_tracking() {
        let tracker = LoginTracker::new(3, 60);
        tracker.record_failure("user@test.com");
        tracker.record_failure("user@test.com");
        tracker.record_success("user@test.com");

        assert!(!tracker.is_locked("user@test.com"));
        // After clearing, it takes 3 more failures to lock
        assert!(!tracker.record_failure("user@test.com"));
        assert!(!tracker.record_failure("user@test.com"));
        assert!(tracker.record_failure("user@test.com"));
    }

    #[test]
    fn case_insensitive() {
        let tracker = LoginTracker::new(3, 60);
        tracker.record_failure("User@Test.Com");
        tracker.record_failure("user@test.com");
        assert!(tracker.record_failure("USER@TEST.COM"));
        assert!(tracker.is_locked("user@test.com"));
    }

    #[test]
    fn lockout_remaining_returns_positive() {
        let tracker = LoginTracker::new(1, 300);
        tracker.record_failure("user@test.com");
        assert!(tracker.lockout_remaining_secs("user@test.com") > 0);
    }
}
