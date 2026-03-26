pub mod auth;
pub mod chat;
pub mod connectors;
pub mod documents;
pub mod feedback;
pub mod health;
pub mod km;
pub mod models;
pub mod settings;
pub mod test_query;
pub mod vault;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, Request};
use axum::middleware;
use axum::{Router, routing::delete, routing::get, routing::post, routing::put};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;

use tower_http::trace::TraceLayer;
use tracing::Span;

use thairag_auth::middleware::auth_layer;

use crate::app_state::AppState;
use crate::csrf::csrf_guard;
use crate::metrics::MetricsLayer;
use crate::rate_limit::{RateLimitLayer, RateLimiter};

async fn metrics_handler(State(state): State<AppState>) -> String {
    state
        .metrics
        .set_active_sessions(state.session_store.count().await);
    state.metrics.encode()
}

pub fn build_router(state: AppState, rate_limiter: Option<RateLimiter>) -> Router {
    // ── Health + metrics routes (never rate-limited) ──
    let health_route = Router::new()
        .route("/health", get(health::health))
        .route("/metrics", get(metrics_handler))
        .with_state(state.clone());

    // ── Public routes (rate-limited) ────────────────────────────────
    let public = Router::new()
        .route("/v1/models", get(models::list_models))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/providers", get(settings::list_enabled_providers))
        .route("/api/auth/ldap", post(auth::ldap_login))
        .route(
            "/api/auth/oauth/{provider_id}/authorize",
            get(auth::oauth_authorize),
        )
        .route("/api/auth/oauth/callback", get(auth::oauth_callback));

    // ── Protected KM routes ─────────────────────────────────────────
    let km_routes = Router::new()
        // Organizations
        .route("/orgs", get(km::list_orgs).post(km::create_org))
        .route("/orgs/{org_id}", get(km::get_org).delete(km::delete_org))
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
        // Users
        .route("/users", get(km::list_users))
        .route("/users/{user_id}", delete(km::delete_user))
        .route("/users/{user_id}/role", put(km::update_user_role))
        // Settings — identity providers
        .route(
            "/settings/identity-providers",
            get(settings::list_identity_providers).post(settings::create_identity_provider),
        )
        .route(
            "/settings/identity-providers/{id}",
            get(settings::get_identity_provider)
                .put(settings::update_identity_provider)
                .delete(settings::delete_identity_provider),
        )
        .route(
            "/settings/identity-providers/{id}/test",
            post(settings::test_idp_connection),
        )
        // Settings — provider config
        .route(
            "/settings/providers",
            get(settings::get_provider_config).put(settings::update_provider_config),
        )
        .route(
            "/settings/providers/models",
            get(settings::list_available_models),
        )
        .route(
            "/settings/providers/models/sync",
            post(settings::sync_models),
        )
        .route(
            "/settings/providers/embedding-models/sync",
            post(settings::sync_embedding_models),
        )
        .route(
            "/settings/providers/reranker-models/sync",
            post(settings::sync_reranker_models),
        )
        // Settings — document processing
        .route(
            "/settings/document",
            get(settings::get_document_config).put(settings::update_document_config),
        )
        // Settings — chat pipeline
        .route(
            "/settings/chat-pipeline",
            get(settings::get_chat_pipeline_config).put(settings::update_chat_pipeline_config),
        )
        // Settings — scoped settings
        .route("/settings/scope-info", get(settings::get_scope_info))
        .route("/settings/scoped", delete(settings::reset_scoped_setting))
        // Settings — presets
        .route("/settings/presets", get(settings::list_presets))
        .route("/settings/presets/apply", post(settings::apply_preset))
        // Settings — Ollama model management
        .route("/settings/ollama/models", get(settings::list_ollama_models))
        .route("/settings/ollama/pull", post(settings::ollama_pull_model))
        // Settings — vector database management
        .route("/settings/vectordb/info", get(settings::get_vectordb_info))
        .route("/settings/vectordb/clear", post(settings::clear_vectordb))
        // Settings — config snapshots
        .route(
            "/settings/snapshots",
            get(settings::list_snapshots).post(settings::create_snapshot),
        )
        .route(
            "/settings/snapshots/{id}/restore",
            post(settings::restore_snapshot),
        )
        .route(
            "/settings/snapshots/{id}",
            delete(settings::delete_snapshot),
        )
        // Settings — prompt management
        .route("/settings/prompts", get(settings::list_prompts))
        .route(
            "/settings/prompts/{key}",
            get(settings::get_prompt)
                .put(settings::update_prompt)
                .delete(settings::delete_prompt_override),
        )
        // Settings — feedback
        .route(
            "/settings/feedback/stats",
            get(feedback::get_feedback_stats),
        )
        .route(
            "/settings/feedback/entries",
            get(feedback::list_feedback_entries),
        )
        .route(
            "/settings/feedback/document-boosts",
            get(feedback::get_document_boosts),
        )
        .route(
            "/settings/feedback/golden-examples",
            get(feedback::list_golden_examples)
                .post(feedback::create_golden_example)
                .delete(feedback::delete_golden_example),
        )
        .route(
            "/settings/feedback/retrieval-params",
            get(feedback::get_retrieval_params).put(feedback::update_retrieval_params),
        )
        // Settings — audit log (OWASP A09)
        .route("/settings/audit-log", get(settings::get_audit_log))
        // Settings — usage stats
        .route("/settings/usage", get(settings::get_usage_stats))
        // Settings — inference logs
        .route(
            "/settings/inference-logs",
            get(settings::list_inference_logs).delete(settings::delete_inference_logs),
        )
        .route(
            "/settings/inference-logs/export",
            get(settings::export_inference_logs),
        )
        .route(
            "/settings/inference-analytics",
            get(settings::get_inference_analytics),
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
            post(documents::upload_document).layer(DefaultBodyLimit::max(
                state.config.document.max_upload_size_mb * 1024 * 1024,
            )),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/content",
            get(documents::get_document_content),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/download",
            get(documents::download_document),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/chunks",
            get(documents::get_document_chunks),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/reprocess",
            post(documents::reprocess_document),
        )
        .route(
            "/workspaces/{workspace_id}/documents/reprocess-all",
            post(documents::reprocess_all_documents),
        )
        // Jobs
        .route("/workspaces/{workspace_id}/jobs", get(documents::list_jobs))
        .route(
            "/workspaces/{workspace_id}/jobs/stream",
            get(documents::stream_jobs),
        )
        .route(
            "/workspaces/{workspace_id}/jobs/{job_id}",
            get(documents::get_job).delete(documents::cancel_job),
        )
        // Test query (search + RAG for a workspace)
        .route(
            "/workspaces/{workspace_id}/test-query",
            post(test_query::test_query),
        )
        .route(
            "/workspaces/{workspace_id}/test-query-stream",
            post(test_query::test_query_stream),
        )
        // MCP Connectors
        .route(
            "/connectors",
            get(connectors::list_connectors).post(connectors::create_connector),
        )
        .route(
            "/connectors/templates",
            get(connectors::list_connector_templates),
        )
        .route(
            "/connectors/from-template",
            post(connectors::create_from_template),
        )
        .route(
            "/connectors/{id}",
            get(connectors::get_connector)
                .put(connectors::update_connector)
                .delete(connectors::delete_connector),
        )
        .route("/connectors/{id}/sync", post(connectors::trigger_sync))
        .route("/connectors/{id}/pause", post(connectors::pause_connector))
        .route(
            "/connectors/{id}/resume",
            post(connectors::resume_connector),
        )
        .route(
            "/connectors/{id}/sync-runs",
            get(connectors::list_sync_runs),
        )
        .route("/connectors/{id}/test", post(connectors::test_connection))
        // API Key Vault + LLM Profiles
        .nest("/settings/vault", vault::routes());

    // Apply auth middleware + CSRF guard to KM routes + chat + feedback
    let server_timeout = std::time::Duration::from_secs(state.config.server.request_timeout_secs);
    let jwt = state.jwt.clone();
    let api_keys = state.api_keys.clone();
    let protected = Router::new()
        .nest("/api/km", km_routes)
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/chat/feedback", post(feedback::submit_feedback))
        .layer(middleware::from_fn(csrf_guard))
        .layer(middleware::from_fn(move |req, next| {
            auth_layer(jwt.clone(), api_keys.clone(), req, next)
        }))
        // Server-side request timeout: returns 408 before reverse proxy 504.
        // For SSE (streaming chat), headers are sent immediately so this
        // timeout only applies to the header phase, not the stream body.
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            server_timeout,
        ));

    // Merge public + protected, optionally with rate limiting
    let rate_limited = if let Some(limiter) = rate_limiter {
        public.merge(protected).layer(RateLimitLayer::new(limiter))
    } else {
        public.merge(protected)
    };

    // ── CORS ─────────────────────────────────────────────────────
    let cors = if state.config.server.cors_origins.is_empty() {
        CorsLayer::permissive()
    } else {
        let origins: Vec<HeaderValue> = state
            .config
            .server
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::header::ACCEPT,
                axum::http::header::ORIGIN,
                axum::http::header::HeaderName::from_static("x-request-id"),
            ])
            .allow_credentials(true)
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
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     _span: &Span| {
                        tracing::info!(
                            status = %response.status(),
                            latency_ms = latency.as_millis(),
                            "response"
                        );
                    },
                ),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        // Security headers (OWASP A05)
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("x-xss-protection"),
            HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        // Content Security Policy (OWASP A05)
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'none'; \
                 script-src 'self'; \
                 style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; \
                 font-src 'self'; \
                 connect-src 'self'; \
                 frame-ancestors 'none'; \
                 base-uri 'self'; \
                 form-action 'self'",
            ),
        ))
        .layer(cors)
        .layer(MetricsLayer::new((*state.metrics).clone()))
        .with_state(state)
}
