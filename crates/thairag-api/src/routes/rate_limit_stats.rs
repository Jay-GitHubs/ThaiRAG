use axum::extract::State;
use axum::{Extension, Json};
use serde::Serialize;

use thairag_auth::AuthClaims;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::rate_limit::{BlockedEvent, IpBucketStats, UserBucketStats};
use crate::routes::settings::require_super_admin;

// ── Response Types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct RateLimitStatsResponse {
    pub global: GlobalStats,
    pub ip_stats: Vec<IpBucketStats>,
    pub user_stats: Vec<UserBucketStats>,
}

#[derive(Serialize)]
pub struct GlobalStats {
    pub ip_rate_limiting_enabled: bool,
    pub total_ip_blocked: u64,
    pub total_user_blocked: u64,
    pub active_ip_limiters: usize,
    pub active_user_limiters: usize,
}

#[derive(Serialize)]
pub struct BlockedEventsResponse {
    pub events: Vec<BlockedEvent>,
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /api/admin/rate-limits/stats
pub async fn get_rate_limit_stats(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<RateLimitStatsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let (ip_rate_limiting_enabled, total_ip_blocked, active_ip_limiters, ip_stats) =
        if let Some(ref limiter) = state.ip_rate_limiter {
            (
                true,
                limiter.blocked_log().total_blocked(),
                limiter.active_count(),
                limiter.ip_stats(20),
            )
        } else {
            (false, 0, 0, vec![])
        };

    let total_user_blocked = state.user_rate_limiter.blocked_log().total_blocked();
    let active_user_limiters = state.user_rate_limiter.active_count();
    let user_stats = state.user_rate_limiter.user_stats(20);

    Ok(Json(RateLimitStatsResponse {
        global: GlobalStats {
            ip_rate_limiting_enabled,
            total_ip_blocked,
            total_user_blocked,
            active_ip_limiters,
            active_user_limiters,
        },
        ip_stats,
        user_stats,
    }))
}

/// GET /api/admin/rate-limits/blocked
pub async fn get_blocked_events(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<BlockedEventsResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let mut events: Vec<BlockedEvent> = Vec::new();

    // Merge blocked events from both IP and user rate limiters
    if let Some(ref limiter) = state.ip_rate_limiter {
        events.extend(limiter.blocked_log().recent());
    }
    events.extend(state.user_rate_limiter.blocked_log().recent());

    // Sort by timestamp descending (most recent first)
    events.sort_by_key(|e| std::cmp::Reverse(e.timestamp));

    // Cap at 100 events total
    events.truncate(100);

    Ok(Json(BlockedEventsResponse { events }))
}
