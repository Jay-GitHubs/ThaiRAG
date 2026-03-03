pub mod auth;
pub mod chat;
pub mod documents;
pub mod health;
pub mod km;
pub mod models;

use axum::extract::DefaultBodyLimit;
use axum::http::Request;
use axum::middleware;
use axum::{Router, routing::get, routing::post};
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::Span;

use thairag_auth::middleware::auth_layer;

use crate::app_state::AppState;
use crate::rate_limit::{RateLimitLayer, RateLimiter};

pub fn build_router(state: AppState, rate_limiter: Option<RateLimiter>) -> Router {
    // ── Health route (never rate-limited) ───────────────────────────
    let health_route = Router::new().route("/health", get(health::health));

    // ── Public routes (rate-limited) ────────────────────────────────
    let public = Router::new()
        .route("/v1/models", get(models::list_models))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login));

    // ── Protected KM routes ─────────────────────────────────────────
    let km_routes = Router::new()
        // Organizations
        .route("/orgs", get(km::list_orgs).post(km::create_org))
        .route(
            "/orgs/{org_id}",
            get(km::get_org).delete(km::delete_org),
        )
        // Departments
        .route(
            "/orgs/{org_id}/depts",
            get(km::list_depts).post(km::create_dept),
        )
        .route(
            "/orgs/{org_id}/depts/{dept_id}",
            get(km::get_dept).delete(km::delete_dept),
        )
        // Workspaces
        .route(
            "/orgs/{org_id}/depts/{dept_id}/workspaces",
            get(km::list_workspaces).post(km::create_workspace),
        )
        .route(
            "/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}",
            get(km::get_workspace).delete(km::delete_workspace),
        )
        // Permissions (org-level)
        .route(
            "/orgs/{org_id}/permissions",
            get(km::list_permissions)
                .post(km::grant_permission)
                .delete(km::revoke_permission),
        )
        // Permissions (dept-level)
        .route(
            "/orgs/{org_id}/depts/{dept_id}/permissions",
            get(km::list_dept_permissions)
                .post(km::grant_dept_permission)
                .delete(km::revoke_dept_permission),
        )
        // Permissions (workspace-level)
        .route(
            "/orgs/{org_id}/depts/{dept_id}/workspaces/{ws_id}/permissions",
            get(km::list_workspace_permissions)
                .post(km::grant_workspace_permission)
                .delete(km::revoke_workspace_permission),
        )
        // Documents
        .route(
            "/workspaces/{workspace_id}/documents",
            get(documents::list_documents).post(documents::ingest_document),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}",
            get(documents::get_document).delete(documents::delete_document),
        )
        .route(
            "/workspaces/{workspace_id}/documents/upload",
            post(documents::upload_document)
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        );

    // Apply auth middleware to KM routes + chat
    let jwt = state.jwt.clone();
    let protected = Router::new()
        .nest("/api/km", km_routes)
        .route("/v1/chat/completions", post(chat::chat_completions))
        .layer(middleware::from_fn(move |req, next| {
            auth_layer(jwt.clone(), req, next)
        }));

    // Merge public + protected, optionally with rate limiting
    let rate_limited = if let Some(limiter) = rate_limiter {
        public
            .merge(protected)
            .layer(RateLimitLayer::new(limiter))
    } else {
        public.merge(protected)
    };

    // health (no rate limit) + rate-limited routes + common layers
    health_route
        .merge(rate_limited)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    let request_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-");
                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id = %request_id,
                    )
                })
                .on_response(|response: &axum::http::Response<_>, latency: std::time::Duration, _span: &Span| {
                    tracing::info!(
                        status = %response.status(),
                        latency_ms = latency.as_millis(),
                        "response"
                    );
                }),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
