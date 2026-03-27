use axum::Extension;
use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use thairag_auth::AuthClaims;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::store::LineageRecord;

use super::settings::require_super_admin;

// ── Query params ──────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct LimitParam {
    pub limit: Option<usize>,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// GET /api/km/lineage/response/{response_id}
pub async fn get_response_lineage(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(response_id): Path<String>,
) -> Result<Json<Vec<LineageRecord>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let records = state.km_store.get_lineage_for_response(&response_id);
    Ok(Json(records))
}

/// GET /api/km/lineage/document/{doc_id}
pub async fn get_document_lineage(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(doc_id): Path<String>,
    Query(params): Query<LimitParam>,
) -> Result<Json<Vec<LineageRecord>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let limit = params.limit.unwrap_or(50).min(500);
    let records = state.km_store.get_lineage_for_document(&doc_id, limit);
    Ok(Json(records))
}
