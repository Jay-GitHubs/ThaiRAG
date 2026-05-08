pub mod ab_test;
pub mod acl;
pub mod api_keys;
pub mod auth;
pub mod backup;
pub mod chat;
pub mod collaboration;
pub mod connectors;
pub mod documents;
pub mod eval;
pub mod feedback;
pub mod finetune;
pub mod guardrails;
pub mod health;
pub mod km;
pub mod knowledge_graph;
pub mod lineage;
pub mod models;
pub mod personal_memory;
pub mod plugins;
pub mod prompt_marketplace;
pub mod rate_limit_stats;
pub mod rbac;
pub mod search_analytics;
pub mod settings;
pub mod tenants;
pub mod test_query;
pub mod v2;
pub mod vault;
pub mod vector_migration;
pub mod webhooks;
pub mod ws_chat;

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, Request};
use axum::middleware;
use axum::{Router, routing::delete, routing::get, routing::patch, routing::post, routing::put};
use sha2::{Digest, Sha256};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;

use tower_http::trace::TraceLayer;
use tracing::Span;

use thairag_auth::middleware::auth_layer;
use thairag_auth::{DynamicApiKeyInfo, DynamicApiKeyValidator};

use crate::app_state::AppState;
use crate::csrf::csrf_guard;
use crate::metrics::MetricsLayer;
use crate::rate_limit::{RateLimitLayer, RateLimiter};
use crate::store::KmStoreTrait;

/// Implements `DynamicApiKeyValidator` using the KM store.
struct StoreApiKeyValidator {
    km_store: Arc<dyn KmStoreTrait>,
}

impl DynamicApiKeyValidator for StoreApiKeyValidator {
    fn validate(&self, raw_key: &str) -> Option<DynamicApiKeyInfo> {
        // Hash the raw key with SHA-256
        let mut hasher = Sha256::new();
        hasher.update(raw_key.as_bytes());
        let key_hash = hex::encode(hasher.finalize());

        let api_key = self.km_store.get_api_key_by_hash(&key_hash)?;
        if !api_key.is_active {
            return None;
        }

        // Update last_used_at timestamp (fire and forget)
        self.km_store.touch_api_key(api_key.id);

        // Look up the owning user for email
        let user = self.km_store.get_user(api_key.user_id).ok()?;

        Some(DynamicApiKeyInfo {
            user_id: api_key.user_id.0.to_string(),
            email: user.email,
        })
    }
}

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
        .route("/v2/models", get(v2::v2_models::list_models_v2))
        .route("/api/version", get(v2::version_info::api_version_info))
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
        .route("/users/{user_id}/status", put(km::update_user_status))
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
        // Guardrails monitoring (PR2)
        .route("/guardrails/stats", get(guardrails::get_stats))
        .route("/guardrails/violations", get(guardrails::list_violations))
        .route("/guardrails/preview", post(guardrails::preview))
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
            "/workspaces/{workspace_id}/documents/batch",
            post(documents::batch_upload_documents).layer(DefaultBodyLimit::max(
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
        // Document Versioning
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/versions",
            get(documents::list_document_versions),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/versions/{version}",
            get(documents::get_document_version),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/diff",
            get(documents::diff_document_versions),
        )
        // Document Refresh Schedule
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/schedule",
            patch(documents::update_document_schedule),
        )
        // Workspace ACLs
        .route(
            "/workspaces/{ws_id}/acl",
            get(acl::list_workspace_acls).post(acl::grant_workspace_acl),
        )
        .route(
            "/workspaces/{ws_id}/acl/{user_id}",
            delete(acl::revoke_workspace_acl),
        )
        // Document ACLs
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/acl",
            post(acl::grant_document_acl),
        )
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/acl/{user_id}",
            delete(acl::revoke_document_acl),
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
        // Streaming reranking search
        .route(
            "/workspaces/{workspace_id}/search-stream",
            post(test_query::search_stream),
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
        .nest("/settings/vault", vault::routes())
        // Webhooks
        .route(
            "/webhooks",
            get(webhooks::list_webhooks).post(webhooks::create_webhook),
        )
        .route("/webhooks/{webhook_id}", delete(webhooks::delete_webhook))
        .route("/webhooks/{webhook_id}/test", post(webhooks::test_webhook))
        // Backup & Restore
        .route("/admin/backup", post(backup::create_backup))
        .route("/admin/restore", post(backup::restore_backup))
        .route("/admin/backup/preview", post(backup::preview_backup))
        // Rate Limit Dashboard
        .route(
            "/admin/rate-limits/stats",
            get(rate_limit_stats::get_rate_limit_stats),
        )
        .route(
            "/admin/rate-limits/blocked",
            get(rate_limit_stats::get_blocked_events),
        )
        // Vector Database Migration
        .route(
            "/admin/vector-migration/start",
            post(vector_migration::start_migration),
        )
        .route(
            "/admin/vector-migration/status",
            get(vector_migration::get_migration_status),
        )
        .route(
            "/admin/vector-migration/validate",
            post(vector_migration::validate_migration),
        )
        .route(
            "/admin/vector-migration/switch",
            post(vector_migration::switch_provider),
        )
        // Search Quality Evaluation
        .route(
            "/eval/query-sets",
            get(eval::list_query_sets).post(eval::create_query_set),
        )
        .route("/eval/query-sets/import", post(eval::import_query_set))
        .route(
            "/eval/query-sets/{id}",
            get(eval::get_query_set).delete(eval::delete_query_set),
        )
        .route("/eval/query-sets/{id}/run", post(eval::run_evaluation))
        .route("/eval/query-sets/{id}/results", get(eval::list_results))
        // Regression
        .route("/eval/regression-check", post(eval::run_regression_check))
        .route(
            "/eval/regression-history",
            get(eval::list_regression_history),
        )
        // Embedding Fine-tuning
        .route(
            "/finetune/datasets",
            get(finetune::list_datasets).post(finetune::create_dataset),
        )
        .route(
            "/finetune/datasets/{id}",
            get(finetune::get_dataset).delete(finetune::delete_dataset),
        )
        .route(
            "/finetune/datasets/{id}/pairs",
            get(finetune::list_pairs).post(finetune::add_pair),
        )
        .route(
            "/finetune/datasets/{id}/pairs/{pair_id}",
            delete(finetune::delete_pair),
        )
        .route(
            "/finetune/datasets/{id}/import-feedback",
            post(finetune::import_feedback),
        )
        .route(
            "/finetune/datasets/{id}/export",
            get(finetune::export_dataset),
        )
        .route(
            "/finetune/jobs",
            get(finetune::list_jobs).post(finetune::create_job),
        )
        .route(
            "/finetune/jobs/{id}",
            get(finetune::get_job).delete(finetune::delete_job),
        )
        .route("/finetune/jobs/{id}/start", post(finetune::start_job))
        .route("/finetune/jobs/{id}/cancel", post(finetune::cancel_job))
        .route("/finetune/jobs/{id}/logs", get(finetune::get_job_logs))
        // Prompt Marketplace
        .route(
            "/prompts/marketplace",
            get(prompt_marketplace::list_templates).post(prompt_marketplace::create_template),
        )
        .route(
            "/prompts/marketplace/{id}",
            get(prompt_marketplace::get_template)
                .put(prompt_marketplace::update_template)
                .delete(prompt_marketplace::delete_template),
        )
        .route(
            "/prompts/marketplace/{id}/rate",
            post(prompt_marketplace::rate_template),
        )
        .route(
            "/prompts/marketplace/{id}/fork",
            post(prompt_marketplace::fork_template),
        )
        // A/B Testing
        .route(
            "/ab-tests",
            get(ab_test::list_ab_tests).post(ab_test::create_ab_test),
        )
        .route(
            "/ab-tests/{id}",
            get(ab_test::get_ab_test).delete(ab_test::delete_ab_test),
        )
        .route("/ab-tests/{id}/run", post(ab_test::run_ab_test))
        .route("/ab-tests/{id}/compare", post(ab_test::compare_ab_test))
        // Plugins
        .route("/plugins", get(plugins::list_plugins))
        .route("/plugins/{name}/enable", post(plugins::enable_plugin))
        .route("/plugins/{name}/disable", post(plugins::disable_plugin))
        // Knowledge Graph
        .route(
            "/workspaces/{workspace_id}/knowledge-graph",
            get(knowledge_graph::get_knowledge_graph),
        )
        .route(
            "/workspaces/{workspace_id}/entities",
            get(knowledge_graph::list_entities),
        )
        .route(
            "/workspaces/{workspace_id}/entities/{entity_id}",
            get(knowledge_graph::get_entity).delete(knowledge_graph::delete_entity),
        )
        .route(
            "/workspaces/{workspace_id}/documents/{doc_id}/extract",
            post(knowledge_graph::extract_from_document),
        )
        // Search Analytics
        .route(
            "/search-analytics/events",
            get(search_analytics::list_search_events),
        )
        .route(
            "/search-analytics/popular",
            get(search_analytics::get_popular_queries),
        )
        .route(
            "/search-analytics/summary",
            get(search_analytics::get_search_summary),
        )
        // Document Lineage
        .route(
            "/lineage/response/{response_id}",
            get(lineage::get_response_lineage),
        )
        .route(
            "/lineage/document/{doc_id}",
            get(lineage::get_document_lineage),
        )
        // Audit Log Export & Analytics
        .route(
            "/settings/audit-log/export",
            get(settings::export_audit_logs),
        )
        .route(
            "/settings/audit-log/analytics",
            get(settings::get_audit_analytics),
        )
        // Personal Memory
        .route(
            "/users/{user_id}/memories",
            get(personal_memory::list_memories).delete(personal_memory::delete_all_memories),
        )
        .route(
            "/users/{user_id}/memories/{memory_id}",
            delete(personal_memory::delete_memory),
        )
        // Multi-tenancy
        .route(
            "/tenants",
            get(tenants::list_tenants).post(tenants::create_tenant),
        )
        .route(
            "/tenants/{id}",
            get(tenants::get_tenant)
                .put(tenants::update_tenant)
                .delete(tenants::delete_tenant),
        )
        .route(
            "/tenants/{id}/quota",
            get(tenants::get_quota).put(tenants::set_quota),
        )
        .route("/tenants/{id}/usage", get(tenants::get_usage))
        .route("/tenants/{id}/assign-org", post(tenants::assign_org))
        // RBAC v2
        .route("/roles", get(rbac::list_roles).post(rbac::create_role))
        .route(
            "/roles/{id}",
            get(rbac::get_role)
                .put(rbac::update_role)
                .delete(rbac::delete_role),
        )
        // Collaboration — Comments
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/comments",
            get(collaboration::list_comments).post(collaboration::create_comment),
        )
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/comments/{comment_id}",
            delete(collaboration::delete_comment),
        )
        // Collaboration — Annotations
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/annotations",
            get(collaboration::list_annotations).post(collaboration::create_annotation),
        )
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/annotations/{annotation_id}",
            delete(collaboration::delete_annotation),
        )
        // Collaboration — Reviews
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/reviews",
            get(collaboration::list_reviews).post(collaboration::create_review),
        )
        .route(
            "/workspaces/{ws_id}/documents/{doc_id}/reviews/{review_id}",
            put(collaboration::update_review_status),
        );

    // Apply auth middleware + CSRF guard to KM routes + chat + feedback
    let server_timeout = std::time::Duration::from_secs(state.config.server.request_timeout_secs);
    let jwt = state.jwt.clone();
    let api_keys = state.api_keys.clone();
    let dynamic_validator: Option<Arc<dyn DynamicApiKeyValidator>> =
        Some(Arc::new(StoreApiKeyValidator {
            km_store: state.km_store.clone(),
        }));

    // ── WebSocket route (auth but no CSRF, no timeout) ─────────────
    let ws_jwt = jwt.clone();
    let ws_api_keys = api_keys.clone();
    let ws_dynamic_validator = dynamic_validator.clone();
    let ws_routes = Router::new()
        .route("/ws/chat", get(ws_chat::ws_chat_handler))
        .layer(middleware::from_fn(move |req, next| {
            auth_layer(
                ws_jwt.clone(),
                ws_api_keys.clone(),
                ws_dynamic_validator.clone(),
                req,
                next,
            )
        }));

    let protected = Router::new()
        .nest("/api/km", km_routes)
        .route(
            "/api/auth/api-keys",
            get(api_keys::list_api_keys).post(api_keys::create_api_key),
        )
        .route(
            "/api/auth/api-keys/{key_id}",
            delete(api_keys::revoke_api_key),
        )
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route(
            "/v2/chat/completions",
            post(v2::v2_chat::v2_chat_completions),
        )
        .route("/v2/search", post(v2::v2_search::v2_search))
        .route(
            "/api/chat/sessions/{session_id}/summary",
            get(chat::get_session_summary),
        )
        .route(
            "/api/chat/sessions/{session_id}/summarize",
            post(chat::summarize_session),
        )
        .route("/v1/chat/feedback", post(feedback::submit_feedback))
        .layer(middleware::from_fn(csrf_guard))
        .layer(middleware::from_fn(move |req, next| {
            auth_layer(
                jwt.clone(),
                api_keys.clone(),
                dynamic_validator.clone(),
                req,
                next,
            )
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
        public
            .merge(protected)
            .merge(ws_routes)
            .layer(RateLimitLayer::new(limiter))
    } else {
        public.merge(protected).merge(ws_routes)
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
                axum::http::Method::PATCH,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::header::ACCEPT,
                axum::http::header::ORIGIN,
                axum::http::header::HeaderName::from_static("x-request-id"),
                axum::http::header::HeaderName::from_static("x-api-key"),
                axum::http::header::HeaderName::from_static("x-api-version"),
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
