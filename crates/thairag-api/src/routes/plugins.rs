use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;

use crate::app_state::AppState;
use crate::plugin_registry::PluginInfo;

#[derive(Serialize)]
pub struct PluginListResponse {
    pub plugins: Vec<PluginInfo>,
}

#[derive(Serialize)]
pub struct PluginActionResponse {
    pub name: String,
    pub enabled: bool,
    pub message: String,
}

/// GET /api/km/plugins — list all registered plugins with their status.
pub async fn list_plugins(State(state): State<AppState>) -> Json<PluginListResponse> {
    let plugins = state.plugin_registry.list_plugins();
    Json(PluginListResponse { plugins })
}

/// POST /api/km/plugins/:name/enable — enable a plugin.
pub async fn enable_plugin(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PluginActionResponse>, StatusCode> {
    if state.plugin_registry.enable(&name) {
        // Persist to KV store
        persist_enabled_state(&state);
        Ok(Json(PluginActionResponse {
            name: name.clone(),
            enabled: true,
            message: format!("Plugin '{name}' enabled"),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /api/km/plugins/:name/disable — disable a plugin.
pub async fn disable_plugin(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PluginActionResponse>, StatusCode> {
    if state.plugin_registry.disable(&name) {
        // Persist to KV store
        persist_enabled_state(&state);
        Ok(Json(PluginActionResponse {
            name: name.clone(),
            enabled: false,
            message: format!("Plugin '{name}' disabled"),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Save current enabled plugin names to the KV store.
fn persist_enabled_state(state: &AppState) {
    let plugins = state.plugin_registry.list_plugins();
    let enabled_names: Vec<String> = plugins
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.name.clone())
        .collect();
    state
        .km_store
        .set_setting("plugins.enabled", &enabled_names.join(","));
}
