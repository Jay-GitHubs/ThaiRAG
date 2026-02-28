use config::{Config, File};
use thairag_core::ThaiRagError;

use crate::schema::AppConfig;

/// Load configuration with layered overrides:
/// 1. config/default.toml — base defaults
/// 2. config/tiers/{tier}.toml — tier preset (optional, via THAIRAG_TIER env)
/// 3. config/local.toml — local overrides (optional)
/// 4. THAIRAG__* environment variables
pub fn load_config() -> std::result::Result<AppConfig, ThaiRagError> {
    let mut builder = Config::builder()
        .add_source(File::with_name("config/default").required(true));

    // Layer 2: tier preset
    if let Ok(tier) = std::env::var("THAIRAG_TIER") {
        builder = builder.add_source(
            File::with_name(&format!("config/tiers/{tier}")).required(false),
        );
    }

    // Layer 3: local overrides
    builder = builder.add_source(File::with_name("config/local").required(false));

    // Layer 4: environment variables (THAIRAG__SERVER__PORT=9090, etc.)
    builder = builder.add_source(
        config::Environment::with_prefix("THAIRAG")
            .separator("__")
            .try_parsing(true),
    );

    let cfg = builder
        .build()
        .map_err(|e| ThaiRagError::Config(e.to_string()))?;

    cfg.try_deserialize::<AppConfig>()
        .map_err(|e| ThaiRagError::Config(e.to_string()))
}
