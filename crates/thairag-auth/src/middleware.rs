use std::collections::HashSet;
use std::sync::Arc;

use axum::{extract::Request, middleware::Next, response::Response};
use thairag_core::ThaiRagError;

use crate::claims::AuthClaims;
use crate::jwt::JwtService;

/// Auth middleware layer for axum.
/// When auth is disabled, injects a default anonymous claim.
/// When enabled, extracts Bearer token and validates as:
///   1. Static API key (if configured) — returns a service account claim
///   2. JWT token — decoded and validated via JwtService
pub async fn auth_layer(
    jwt: Option<Arc<JwtService>>,
    api_keys: Arc<HashSet<String>>,
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
            // Try Authorization header first, then fall back to ?token= query
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
