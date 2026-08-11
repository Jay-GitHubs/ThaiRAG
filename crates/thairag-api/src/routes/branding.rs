//! White-label branding: operator-configurable product name + logo, served to
//! both UIs (admin + chat). Persisted as the `branding` KV setting and read
//! back through a PUBLIC endpoint — the login screens need it before anyone
//! authenticates. Editing requires super-admin.

use axum::Json;
use axum::extract::Extension;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;

/// Max size of a logo data URL (bytes). Keeps the branding payload small — it
/// is fetched unauthenticated on every page load. ~200 KB covers any
/// reasonable PNG/SVG logo.
const MAX_LOGO_BYTES: usize = 200 * 1024;
const MAX_APP_NAME_LEN: usize = 60;

fn default_app_name() -> String {
    "ThaiRAG".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrandingConfig {
    /// Product name shown in headers, login screens, and browser titles.
    #[serde(default = "default_app_name")]
    pub app_name: String,
    /// Optional logo as a `data:image/...;base64,...` URL. `None` = the
    /// default text/icon header. Kept inline (not a file) so it rides the
    /// same KV-setting + effective-config path as everything else.
    #[serde(default)]
    pub logo_data_url: Option<String>,
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            app_name: default_app_name(),
            logo_data_url: None,
        }
    }
}

impl BrandingConfig {
    /// Validate operator input. Errors are safe to show (no upstream secrets).
    pub fn validate(&self) -> Result<(), String> {
        let name = self.app_name.trim();
        if name.is_empty() {
            return Err("app_name must not be empty".into());
        }
        if name.chars().count() > MAX_APP_NAME_LEN {
            return Err(format!(
                "app_name must be at most {MAX_APP_NAME_LEN} characters"
            ));
        }
        if let Some(ref logo) = self.logo_data_url
            && !logo.is_empty()
        {
            if !logo.starts_with("data:image/") {
                return Err("logo_data_url must be a data:image/... URL".into());
            }
            if logo.len() > MAX_LOGO_BYTES {
                return Err(format!(
                    "logo is too large ({} bytes); max {} KB",
                    logo.len(),
                    MAX_LOGO_BYTES / 1024
                ));
            }
        }
        Ok(())
    }

    /// Normalize: trim the name; treat an empty logo string as None.
    fn normalized(mut self) -> Self {
        self.app_name = self.app_name.trim().to_string();
        if self.logo_data_url.as_deref().is_some_and(str::is_empty) {
            self.logo_data_url = None;
        }
        self
    }
}

/// Read the effective branding (persisted setting, or defaults).
pub fn effective_branding(state: &AppState) -> BrandingConfig {
    state
        .km_store
        .get_setting("branding")
        .and_then(|s| serde_json::from_str::<BrandingConfig>(&s).ok())
        .unwrap_or_default()
}

/// GET /api/branding — PUBLIC (login screens need it pre-auth).
pub async fn get_branding(State(state): State<AppState>) -> Json<BrandingConfig> {
    Json(effective_branding(&state))
}

/// PUT /api/km/settings/branding — super-admin only.
pub async fn update_branding(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<BrandingConfig>,
) -> Result<Json<BrandingConfig>, ApiError> {
    crate::routes::settings::require_super_admin(&claims, &state)?;
    let cfg = body.normalized();
    cfg.validate()
        .map_err(|e| ApiError(ThaiRagError::Validation(e)))?;
    let json = serde_json::to_string(&cfg)
        .map_err(|e| ApiError(ThaiRagError::Internal(format!("serialize branding: {e}"))))?;
    state.km_store.set_setting("branding", &json);
    Ok(Json(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_thairag_no_logo() {
        let b = BrandingConfig::default();
        assert_eq!(b.app_name, "ThaiRAG");
        assert!(b.logo_data_url.is_none());
    }

    #[test]
    fn validate_rejects_empty_and_overlong_name() {
        let mut b = BrandingConfig::default();
        b.app_name = "  ".into();
        assert!(b.validate().is_err());
        b.app_name = "x".repeat(MAX_APP_NAME_LEN + 1);
        assert!(b.validate().is_err());
        b.app_name = "Acme Knowledge".into();
        assert!(b.validate().is_ok());
    }

    #[test]
    fn validate_rejects_non_data_and_oversized_logo() {
        let mut b = BrandingConfig::default();
        b.logo_data_url = Some("https://evil.example/logo.png".into());
        assert!(
            b.validate().is_err(),
            "must reject non-data URLs (SSRF/leak)"
        );
        b.logo_data_url = Some(format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_LOGO_BYTES)
        ));
        assert!(b.validate().is_err(), "must reject oversized logo");
        b.logo_data_url = Some("data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=".into());
        assert!(b.validate().is_ok());
    }

    #[test]
    fn normalized_trims_name_and_empties_logo_to_none() {
        let b = BrandingConfig {
            app_name: "  Acme  ".into(),
            logo_data_url: Some(String::new()),
        }
        .normalized();
        assert_eq!(b.app_name, "Acme");
        assert!(b.logo_data_url.is_none());
    }

    #[test]
    fn roundtrips_and_missing_fields_default() {
        // Old/partial stored JSON must deserialize with defaults.
        let parsed: BrandingConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, BrandingConfig::default());
        let parsed: BrandingConfig = serde_json::from_str(r#"{"app_name":"Acme"}"#).unwrap();
        assert_eq!(parsed.app_name, "Acme");
        assert!(parsed.logo_data_url.is_none());
    }
}
