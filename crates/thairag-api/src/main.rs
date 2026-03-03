use std::net::SocketAddr;
use std::time::Duration;

use tokio::signal;
use tracing_subscriber::EnvFilter;

use thairag_api::app_state::AppState;
use thairag_api::routes::build_router;

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
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
    }

    // Load config
    let config = thairag_config::load_config().expect("Failed to load configuration");

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let shutdown_timeout = Duration::from_secs(config.server.shutdown_timeout_secs);

    // Build app state with all providers wired
    let state = AppState::build(config);

    // Build router
    let app = build_router(state);

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

    tracing::info!("Server shutdown complete");
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
