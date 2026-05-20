use axum::Extension;
use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::plugin_registry::PluginInfo;
use crate::routes::settings::require_super_admin;

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
    Extension(claims): Extension<AuthClaims>,
    Path(name): Path<String>,
) -> Result<Json<PluginActionResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    if state.plugin_registry.enable(&name) {
        // Persist to KV store
        persist_enabled_state(&state);
        Ok(Json(PluginActionResponse {
            name: name.clone(),
            enabled: true,
            message: format!("Plugin '{name}' enabled"),
        }))
    } else {
        Err(ThaiRagError::NotFound(format!("Plugin '{name}' not found")).into())
    }
}

/// POST /api/km/plugins/:name/disable — disable a plugin.
pub async fn disable_plugin(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(name): Path<String>,
) -> Result<Json<PluginActionResponse>, ApiError> {
    require_super_admin(&claims, &state)?;
    if state.plugin_registry.disable(&name) {
        // Persist to KV store
        persist_enabled_state(&state);
        Ok(Json(PluginActionResponse {
            name: name.clone(),
            enabled: false,
            message: format!("Plugin '{name}' disabled"),
        }))
    } else {
        Err(ThaiRagError::NotFound(format!("Plugin '{name}' not found")).into())
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
