use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app_state::AppState;

#[derive(Deserialize, Default)]
pub struct HealthQuery {
    #[serde(default)]
    pub deep: Option<bool>,
}

pub async fn health(
    State(state): State<AppState>,
    Query(query): Query<HealthQuery>,
) -> Response {
    if query.deep.unwrap_or(false) {
        deep_health(state).await
    } else {
        Json(json!({
            "status": "ok",
            "service": "thairag",
            "version": env!("CARGO_PKG_VERSION"),
        }))
        .into_response()
    }
}

async fn deep_health(state: AppState) -> Response {
    let mut checks: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut all_ok = true;

    // Probe embedding provider
    let embedding_ok = state
        .providers()
        .embedding
        .embed(&["health check".to_string()])
        .await
        .is_ok();
    checks.insert(
        "embedding".to_string(),
        json!(if embedding_ok { "ok" } else { "fail" }),
    );
    if !embedding_ok {
        all_ok = false;
    }

    let status_str = if all_ok { "ok" } else { "degraded" };
    let status_code = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let body = json!({
        "status": status_str,
        "service": "thairag",
        "version": env!("CARGO_PKG_VERSION"),
        "checks": checks,
    });

    (status_code, Json(body)).into_response()
}
