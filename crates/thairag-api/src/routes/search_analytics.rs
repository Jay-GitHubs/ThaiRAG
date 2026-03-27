use axum::Extension;
use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;

use thairag_auth::AuthClaims;

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

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/km/search-analytics/events
pub async fn list_search_events(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(filter): Query<SearchAnalyticsFilter>,
) -> Result<Json<Vec<SearchAnalyticsEvent>>, ApiError> {
    require_super_admin(&claims, &state)?;
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
    let summary = state.km_store.get_search_analytics_summary(&filter);
    Ok(Json(summary))
}
