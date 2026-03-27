//! API version info endpoint.
//!
//! `GET /api/version`
//!
//! Returns supported API versions, current default, and deprecation notices.

use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiVersionInfo {
    pub current_version: String,
    pub default_version: String,
    pub supported_versions: Vec<VersionEntry>,
}

#[derive(Debug, Serialize)]
pub struct VersionEntry {
    pub version: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecation_notice: Option<String>,
    pub endpoints: Vec<String>,
}

/// GET /api/version
///
/// Returns information about supported API versions.
pub async fn api_version_info() -> Json<ApiVersionInfo> {
    Json(ApiVersionInfo {
        current_version: "v2".to_string(),
        default_version: "v1".to_string(),
        supported_versions: vec![
            VersionEntry {
                version: "v1".to_string(),
                status: "stable".to_string(),
                deprecation_notice: None,
                endpoints: vec![
                    "GET  /v1/models".to_string(),
                    "POST /v1/chat/completions".to_string(),
                    "POST /v1/chat/feedback".to_string(),
                ],
            },
            VersionEntry {
                version: "v2".to_string(),
                status: "stable".to_string(),
                deprecation_notice: None,
                endpoints: vec![
                    "GET  /v2/models".to_string(),
                    "POST /v2/chat/completions".to_string(),
                    "POST /v2/search".to_string(),
                ],
            },
        ],
    })
}
