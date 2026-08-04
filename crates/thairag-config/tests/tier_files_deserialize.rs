//! Every shipped tier preset must deserialize into AppConfig when layered on
//! default.toml — the same merge the runtime loader performs. A typo'd enum
//! value in a tier file (e.g. `kind = "openai"` instead of `open_ai`) makes
//! that tier panic at boot on any deployment that doesn't happen to override
//! the bad key, which is exactly how it slipped through until a clean VM
//! deploy hit it.

use config::{Config, File};
use thairag_config::AppConfig;

fn config_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config")
}

#[test]
fn every_tier_preset_deserializes_on_top_of_defaults() {
    let dir = config_dir();
    let default = dir.join("default.toml");
    assert!(
        default.exists(),
        "config/default.toml missing at {default:?}"
    );

    let tier_files: Vec<_> = std::fs::read_dir(dir.join("tiers"))
        .expect("config/tiers/ directory missing")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(!tier_files.is_empty(), "no tier presets found");

    for tier in tier_files {
        let cfg = Config::builder()
            .add_source(File::from(default.as_path()).required(true))
            .add_source(File::from(tier.as_path()).required(true))
            .build()
            .unwrap_or_else(|e| panic!("failed to merge {tier:?}: {e}"));

        cfg.try_deserialize::<AppConfig>()
            .unwrap_or_else(|e| panic!("tier preset {tier:?} does not deserialize: {e}"));
    }
}
