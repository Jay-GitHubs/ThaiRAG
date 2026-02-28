mod app_state;
mod error;
mod routes;

use tracing_subscriber::EnvFilter;

use app_state::AppState;
use routes::build_router;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Load config
    let config = thairag_config::load_config().expect("Failed to load configuration");

    let addr = format!("{}:{}", config.server.host, config.server.port);

    // Build app state with all providers wired
    let state = AppState::build(config);

    // Build router
    let app = build_router(state);

    // Serve
    tracing::info!("ThaiRAG server starting on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
