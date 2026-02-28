pub mod chat;
pub mod documents;
pub mod health;
pub mod km;
pub mod models;

use axum::{Router, routing::get, routing::post};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app_state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/models", get(models::list_models))
        // Health
        .route("/health", get(health::health))
        // KM management (stubs)
        .route("/api/km/orgs", get(km::list_orgs).post(km::create_org))
        // Document ingestion
        .route(
            "/api/km/workspaces/{workspace_id}/documents",
            post(documents::ingest_document),
        )
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
