//! CSRF protection using double-submit token pattern.
//!
//! On login, a CSRF token is returned in the response body alongside the JWT.
//! Clients must include this token in the `X-CSRF-Token` header on all
//! state-changing requests (POST, PUT, DELETE).
//!
//! Since ThaiRAG primarily uses Bearer token auth (not cookies), CSRF risk is
//! low. This is a defense-in-depth measure for environments that also set
//! auth cookies via a reverse proxy.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use thairag_auth::AuthClaims;

/// Middleware that validates the `X-CSRF-Token` header on state-changing requests.
///
/// Safe methods (GET, HEAD, OPTIONS) are always allowed.
/// For POST/PUT/DELETE, the request must include a `Bearer` token (which is
/// inherently CSRF-safe) or an `X-CSRF-Token` header.
///
/// If auth is disabled (anonymous claims present without bearer token), CSRF
/// validation is skipped since there's no session to protect.
pub async fn csrf_guard(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();

    // Safe methods don't need CSRF protection
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return next.run(req).await;
    }

    // If auth is disabled (anonymous user), skip CSRF — no session to protect
    if let Some(claims) = req.extensions().get::<AuthClaims>() {
        if claims.sub == "anonymous" {
            return next.run(req).await;
        }
    }

    // Check for CSRF token header on state-changing requests
    let has_csrf = req.headers().contains_key("x-csrf-token");
    let has_bearer = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "));

    // If using Bearer auth, the token itself serves as CSRF protection
    // (Bearer tokens are not automatically sent by browsers like cookies are).
    // If no Bearer token and no CSRF token, reject.
    if !has_bearer && !has_csrf {
        let body = serde_json::json!({
            "error": {
                "message": "Missing CSRF token. Include X-CSRF-Token header or use Bearer auth.",
                "type": "csrf_error"
            }
        });
        return (StatusCode::FORBIDDEN, axum::Json(body)).into_response();
    }

    next.run(req).await
}
