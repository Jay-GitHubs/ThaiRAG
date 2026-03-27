use std::collections::HashSet;
use std::sync::Arc;

use axum::{extract::Request, middleware::Next, response::Response};
use thairag_core::ThaiRagError;

use crate::claims::AuthClaims;
use crate::jwt::JwtService;

/// Result of validating a dynamic (database-backed) API key.
#[derive(Debug, Clone)]
pub struct DynamicApiKeyInfo {
    /// User ID that owns this API key (stored as `sub` in claims).
    pub user_id: String,
    /// Email of the owning user.
    pub email: String,
}

/// Trait for looking up database-backed API keys (M2M auth).
/// Implemented in `thairag-api` where the store lives.
pub trait DynamicApiKeyValidator: Send + Sync {
    /// Given a raw API key (e.g. `trag_...`), hash it and look up in the store.
    /// Returns user info if the key is valid and active, None otherwise.
    fn validate(&self, raw_key: &str) -> Option<DynamicApiKeyInfo>;
}

/// Auth middleware layer for axum.
/// When auth is disabled, injects a default anonymous claim.
/// When enabled, validates credentials in priority order:
///   1. Bearer JWT token (Authorization header)
///   2. X-API-Key header (database-backed M2M keys)
///   3. Static API keys (Bearer token matching config list)
///   4. ?token= query param (SSE fallback)
pub async fn auth_layer(
    jwt: Option<Arc<JwtService>>,
    api_keys: Arc<HashSet<String>>,
    dynamic_api_key_validator: Option<Arc<dyn DynamicApiKeyValidator>>,
    mut req: Request,
    next: Next,
) -> Result<Response, AuthError> {
    let claims = match &jwt {
        None => {
            // Auth disabled — inject anonymous claims
            AuthClaims {
                sub: "anonymous".into(),
                email: "anonymous@local".into(),
                exp: 0,
                iat: 0,
            }
        }
        Some(jwt_service) => {
            // Check X-API-Key header first (M2M auth)
            if let Some(api_key_header) =
                req.headers().get("x-api-key").and_then(|v| v.to_str().ok())
            {
                if let Some(ref validator) = dynamic_api_key_validator {
                    if let Some(info) = validator.validate(api_key_header) {
                        AuthClaims {
                            sub: info.user_id,
                            email: info.email,
                            exp: usize::MAX,
                            iat: 0,
                        }
                    } else {
                        return Err(AuthError(ThaiRagError::Auth(
                            "Invalid or revoked API key".into(),
                        )));
                    }
                } else {
                    return Err(AuthError(ThaiRagError::Auth(
                        "API key authentication is not configured".into(),
                    )));
                }
            } else {
                // Try Authorization header, then fall back to ?token= query
                // parameter (needed for SSE EventSource which can't set headers).
                let token = if let Some(auth_header) = req
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                {
                    auth_header.strip_prefix("Bearer ").ok_or_else(|| {
                        AuthError(ThaiRagError::Auth("Invalid authorization format".into()))
                    })?
                } else {
                    // Fall back to query parameter for SSE endpoints
                    req.uri()
                        .query()
                        .and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("token=")))
                        .ok_or_else(|| {
                            AuthError(ThaiRagError::Auth("Missing authorization header".into()))
                        })?
                };

                // Check static API keys first
                if !api_keys.is_empty() && api_keys.contains(token) {
                    AuthClaims {
                        sub: "api-key".into(),
                        email: "service@api-key".into(),
                        exp: usize::MAX,
                        iat: 0,
                    }
                } else {
                    jwt_service.decode(token).map_err(AuthError)?
                }
            }
        }
    };

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

#[derive(Debug)]
pub struct AuthError(pub ThaiRagError);

impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": {
                "message": self.0.to_string(),
                "type": "authentication_error",
            }
        });
        (axum::http::StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
    }
}
