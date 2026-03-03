pub mod auth;
pub mod chat;
pub mod documents;
pub mod health;
pub mod km;
pub mod models;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::{Router, routing::get, routing::post};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use thairag_auth::middleware::auth_layer;

use crate::app_state::AppState;

pub fn build_router(state: AppState) -> Router {
    // ── Public routes ───────────────────────────────────────────────
    let public = Router::new()
        .route("/health", get(health::health))
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
        // Permissions
        .route(
            "/orgs/{org_id}/permissions",
            get(km::list_permissions)
                .post(km::grant_permission)
                .delete(km::revoke_permission),
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

    // Merge public + protected
    public
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
