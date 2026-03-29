use axum::Extension;
use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::store::{
    PopularQuery, SearchAnalyticsEvent, SearchAnalyticsFilter, SearchAnalyticsSummary,
};

use super::settings::require_super_admin;

// ── Query params ──────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct LimitParam {
    pub limit: Option<usize>,
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Validate an optional date string is ISO-8601 date format (YYYY-MM-DD) or RFC-3339 datetime.
/// Accepts None (no constraint) or a string that parses as NaiveDate or DateTime.
fn validate_date_param(value: &Option<String>, field: &str) -> Result<(), ApiError> {
    if let Some(s) = value {
        // Accept YYYY-MM-DD or full RFC-3339 / ISO-8601 datetime
        let is_date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok();
        let is_datetime = chrono::DateTime::parse_from_rfc3339(s).is_ok();
        if !is_date && !is_datetime {
            return Err(ApiError(ThaiRagError::Validation(format!(
                "'{field}' must be a valid ISO-8601 date (YYYY-MM-DD) or RFC-3339 datetime, got: {s}"
            ))));
        }
    }
    Ok(())
}

fn validate_analytics_filter(filter: &SearchAnalyticsFilter) -> Result<(), ApiError> {
    validate_date_param(&filter.from, "from")?;
    validate_date_param(&filter.to, "to")?;
    Ok(())
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/km/search-analytics/events
pub async fn list_search_events(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(filter): Query<SearchAnalyticsFilter>,
) -> Result<Json<Vec<SearchAnalyticsEvent>>, ApiError> {
    require_super_admin(&claims, &state)?;
    validate_analytics_filter(&filter)?;
    let events = state.km_store.list_search_events(&filter);
    Ok(Json(events))
}

/// GET /api/km/search-analytics/popular
pub async fn get_popular_queries(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<LimitParam>,
) -> Result<Json<Vec<PopularQuery>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let limit = params.limit.unwrap_or(20).min(100);
    let queries = state.km_store.get_popular_queries(limit);
    Ok(Json(queries))
}

/// GET /api/km/search-analytics/summary
pub async fn get_search_summary(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(filter): Query<SearchAnalyticsFilter>,
) -> Result<Json<SearchAnalyticsSummary>, ApiError> {
    require_super_admin(&claims, &state)?;
    validate_analytics_filter(&filter)?;
    let summary = state.km_store.get_search_analytics_summary(&filter);
    Ok(Json(summary))
}
