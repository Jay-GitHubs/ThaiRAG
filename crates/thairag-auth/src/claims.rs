use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    pub sub: String,
    pub email: String,
    pub exp: usize,
    pub iat: usize,
    /// Database-backed API key id when the request authenticated via
    /// `X-API-Key` (or "static" for the config-list key); `None` for
    /// interactive JWT logins. `#[serde(default)]` so JWTs minted before
    /// this field still deserialize, and it is omitted from JWT claims when
    /// absent (interactive tokens are unchanged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_id: Option<String>,
}
