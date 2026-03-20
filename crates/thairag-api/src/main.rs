use std::net::SocketAddr;
use std::time::Duration;

use tokio::signal;
use tracing_subscriber::EnvFilter;

use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHasher};

use thairag_api::app_state::AppState;
use thairag_api::rate_limit::RateLimiter;
use thairag_api::routes::build_router;
use thairag_api::store::KmStoreTrait;

#[tokio::main]
async fn main() {
    // Initialize tracing
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let log_format = std::env::var("THAIRAG_LOG_FORMAT").unwrap_or_default();

    if log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    // Load and validate config
    let config = thairag_config::load_config().expect("Failed to load configuration");
    config.validate().expect("Invalid configuration");

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let shutdown_timeout = Duration::from_secs(config.server.shutdown_timeout_secs);

    // Build app state with all providers wired
    let state = AppState::build(config.clone());

    // Seed super admin from env vars
    seed_super_admin(&*state.km_store);

    // Load saved config from DB (if any) and hot-reload.
    // Use get_effective_chat_pipeline to include runtime settings (e.g., context
    // compaction, personal memory) that were enabled via the Admin UI.
    {
        let effective_chat = thairag_api::routes::settings::get_effective_chat_pipeline(&state);
        let saved_providers = state
            .km_store
            .get_setting("provider_config")
            .and_then(|s| serde_json::from_str::<thairag_config::schema::ProvidersConfig>(&s).ok());
        let pc = if let Some(ref pc) = saved_providers {
            let mut validate_cfg = config.clone();
            validate_cfg.providers = pc.clone();
            if let Err(e) = validate_cfg.validate() {
                tracing::warn!("Saved provider config is invalid, ignoring: {e}");
                &config.providers
            } else {
                pc
            }
        } else {
            &config.providers
        };
        let bundle = thairag_api::app_state::ProviderBundle::build(
            pc,
            &config.search,
            &config.document,
            &effective_chat,
        );
        state.reload_providers(bundle);
        tracing::info!("Loaded saved config from database");
    }

    // Rebuild Tantivy text search index if empty but DB has chunks
    {
        let p = state.providers();
        let tantivy_count = p.search_engine.text_search_doc_count();
        if tantivy_count == 0 {
            let chunks = state.km_store.load_all_chunks();
            if !chunks.is_empty() {
                tracing::info!(
                    chunk_count = chunks.len(),
                    "Tantivy index is empty — rebuilding from stored chunks"
                );
                // Index in batches to avoid memory pressure
                let batch_size = 500;
                for batch in chunks.chunks(batch_size) {
                    if let Err(e) = p.search_engine.reindex_text_search(batch).await {
                        tracing::error!(error = %e, "Failed to rebuild Tantivy index batch");
                        break;
                    }
                }
                let rebuilt = p.search_engine.text_search_doc_count();
                tracing::info!(doc_count = rebuilt, "Tantivy index rebuild complete");
            }
        } else {
            tracing::info!(doc_count = tantivy_count, "Tantivy index already populated");
        }
    }

    // Create rate limiter (if enabled) and spawn background cleanup
    let rate_limiter = if config.server.rate_limit.enabled {
        Some(
            RateLimiter::new(
                config.server.rate_limit.requests_per_second,
                config.server.rate_limit.burst_size,
            )
            .with_trust_proxy(config.server.trust_proxy),
        )
    } else {
        None
    };

    if let Some(ref limiter) = rate_limiter {
        let limiter = limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                limiter.cleanup_stale(Duration::from_secs(3600));
            }
        });
    }

    // Spawn user rate limiter cleanup task
    {
        let user_rl = state.user_rate_limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                user_rl.cleanup_stale(Duration::from_secs(3600));
            }
        });
    }

    // Spawn session cleanup task (evict sessions idle > 1 hour)
    {
        let session_store = state.session_store.clone();
        let oidc_cache = state.oidc_state_cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600));
            loop {
                interval.tick().await;
                session_store.cleanup_stale(Duration::from_secs(3600));
                oidc_cache.cleanup_stale(Duration::from_secs(600));
            }
        });
    }

    // Start MCP sync scheduler (if enabled)
    let sync_scheduler = if config.mcp.enabled {
        use thairag_api::routes::connectors::{DocumentIngester, StoreAdapter};

        let engine = std::sync::Arc::new(thairag_mcp::SyncEngine::new(
            config.mcp.max_resource_size_bytes,
            config.mcp.sync_retry_max_attempts,
            config.mcp.sync_retry_base_delay_secs,
            config.mcp.sync_retry_max_delay_secs,
        ));
        let store_adapter: std::sync::Arc<dyn thairag_mcp::sync_engine::SyncStore> =
            std::sync::Arc::new(StoreAdapter(state.km_store.clone()));
        let ingester: std::sync::Arc<dyn thairag_mcp::sync_engine::ContentIngester> =
            std::sync::Arc::new(DocumentIngester {
                state: state.clone(),
            });

        // Wire metrics callback for scheduled syncs
        let metrics = state.metrics.clone();
        let on_sync_complete: thairag_mcp::sync_scheduler::SyncRunCallback = std::sync::Arc::new(
            move |name, status, duration, created, updated, skipped, failed| {
                metrics.record_sync_run(name, status, duration, created, updated, skipped, failed);
            },
        );

        let scheduler = std::sync::Arc::new(
            thairag_mcp::SyncScheduler::new(engine, store_adapter, ingester)
                .with_on_sync_complete(on_sync_complete),
        );

        // Load all connectors and start scheduled ones
        let connectors = state.km_store.list_connectors();
        let sched = scheduler.clone();
        tokio::spawn(async move {
            sched.start(connectors).await;
        });

        Some(scheduler)
    } else {
        None
    };

    // Build router
    let app = build_router(state, rate_limiter);

    // Serve
    tracing::info!("ThaiRAG server starting on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(shutdown_timeout))
    .await
    .expect("Server error");

    // Gracefully shut down MCP sync scheduler
    if let Some(scheduler) = sync_scheduler {
        scheduler.shutdown().await;
    }

    tracing::info!("Server shutdown complete");
}

fn seed_super_admin(store: &dyn KmStoreTrait) {
    let email = std::env::var("THAIRAG__ADMIN__EMAIL").unwrap_or_default();
    let password = std::env::var("THAIRAG__ADMIN__PASSWORD").unwrap_or_default();

    if email.is_empty() || password.is_empty() {
        return;
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = match Argon2::default().hash_password(password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(e) => {
            tracing::error!("Failed to hash super admin password: {e}");
            return;
        }
    };

    match store.upsert_user_by_email(
        email.clone(),
        "Super Admin".into(),
        password_hash,
        true,
        "super_admin".into(),
    ) {
        Ok(user) => {
            tracing::info!(email = %email, user_id = %user.id, "Super admin seeded");
        }
        Err(e) => {
            tracing::error!("Failed to seed super admin: {e}");
        }
    }
}

async fn shutdown_signal(timeout: Duration) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!(
        timeout_secs = timeout.as_secs(),
        "Shutdown signal received, starting graceful shutdown"
    );
}
