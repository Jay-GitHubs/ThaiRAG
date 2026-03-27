//! API versioning support.
//!
//! Extracts the API version from:
//! 1. URL path prefix (`/v1/...`, `/v2/...`)
//! 2. `X-API-Version` header (`v1`, `v2`)
//! 3. Defaults to V1 for backward compatibility

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};

/// Supported API versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiVersion {
    V1,
    V2,
}

impl ApiVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }

    /// Parse from a string like "v1", "v2", "V1", "V2".
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "v1" => Some(Self::V1),
            "v2" => Some(Self::V2),
            _ => None,
        }
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Axum extractor that resolves the API version from the request.
///
/// Resolution order:
/// 1. URL path prefix: `/v2/chat/completions` -> V2
/// 2. `X-API-Version` header: `v2` -> V2
/// 3. Default: V1
impl<S: Send + Sync> FromRequestParts<S> for ApiVersion {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 1. Check URL path prefix
        let path = parts.uri.path();
        if path.starts_with("/v2/") || path == "/v2" {
            return Ok(ApiVersion::V2);
        }
        if path.starts_with("/v1/") || path == "/v1" {
            return Ok(ApiVersion::V1);
        }

        // 2. Check X-API-Version header
        if let Some(header_val) = parts.headers.get("x-api-version")
            && let Ok(s) = header_val.to_str()
            && let Some(version) = ApiVersion::from_str_opt(s)
        {
            return Ok(version);
        }

        // 3. Default to V1
        Ok(ApiVersion::V1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_strings() {
        assert_eq!(ApiVersion::from_str_opt("v1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_str_opt("v2"), Some(ApiVersion::V2));
        assert_eq!(ApiVersion::from_str_opt("V1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_str_opt("V2"), Some(ApiVersion::V2));
        assert_eq!(ApiVersion::from_str_opt("v3"), None);
        assert_eq!(ApiVersion::from_str_opt(""), None);
    }

    #[test]
    fn display() {
        assert_eq!(ApiVersion::V1.to_string(), "v1");
        assert_eq!(ApiVersion::V2.to_string(), "v2");
    }
}
