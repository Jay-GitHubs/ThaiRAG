use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use openidconnect::core::{CoreProviderMetadata, CoreResponseType, CoreTokenResponse};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use thairag_core::ThaiRagError;

type Result<T> = std::result::Result<T, ThaiRagError>;

// ── OIDC State Cache ────────────────────────────────────────────────

#[derive(Clone)]
pub struct OidcPendingAuth {
    pub provider_id: String,
    pub pkce_verifier: String,
    pub nonce: String,
    pub created_at: Instant,
}

#[derive(Clone, Default)]
pub struct OidcStateCache {
    inner: Arc<DashMap<String, OidcPendingAuth>>,
}

impl OidcStateCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    pub fn store(&self, state: String, pending: OidcPendingAuth) {
        self.inner.insert(state, pending);
    }

    pub fn take(&self, state: &str) -> Option<OidcPendingAuth> {
        self.inner.remove(state).map(|(_, v)| v)
    }

    pub fn cleanup_stale(&self, max_age: Duration) {
        let now = Instant::now();
        self.inner
            .retain(|_, v| now.duration_since(v.created_at) < max_age);
    }
}

// ── OIDC User Info ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct OidcUserInfo {
    pub external_id: String,
    pub email: String,
    pub name: String,
    pub roles: Vec<String>,
}

// ── OIDC Config from IdP ────────────────────────────────────────────

pub struct OidcProviderConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Vec<String>,
    pub redirect_uri: String,
    /// Maps Keycloak role names to ThaiRAG roles (super_admin, admin, editor, viewer).
    /// Example: {"thairag-admin": "admin", "thairag-editor": "editor"}
    pub role_mapping: std::collections::HashMap<String, String>,
}

impl OidcProviderConfig {
    pub fn from_json(config: &serde_json::Value) -> Result<Self> {
        let issuer_url = config["issuer_url"]
            .as_str()
            .ok_or_else(|| ThaiRagError::Validation("Missing issuer_url in OIDC config".into()))?
            .to_string();
        let client_id = config["client_id"]
            .as_str()
            .ok_or_else(|| ThaiRagError::Validation("Missing client_id in OIDC config".into()))?
            .to_string();
        let client_secret = config["client_secret"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let scopes_str = config["scopes"].as_str().unwrap_or("openid profile email");
        let scopes = scopes_str.split_whitespace().map(String::from).collect();
        let redirect_uri = config["redirect_uri"]
            .as_str()
            .ok_or_else(|| ThaiRagError::Validation("Missing redirect_uri in OIDC config".into()))?
            .to_string();

        let role_mapping: std::collections::HashMap<String, String> = config
            .get("role_mapping")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        Ok(Self {
            issuer_url,
            client_id,
            client_secret,
            scopes,
            redirect_uri,
            role_mapping,
        })
    }
}

// ── Build Authorization URL ─────────────────────────────────────────

pub struct AuthorizeResult {
    pub authorize_url: String,
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
}

/// Perform OIDC discovery and build an authorization URL.
pub async fn build_authorize_url(config: &OidcProviderConfig) -> Result<AuthorizeResult> {
    let issuer_url = IssuerUrl::new(config.issuer_url.clone())
        .map_err(|e| ThaiRagError::Config(format!("Invalid issuer URL: {e}")))?;
    let redirect_url = RedirectUrl::new(config.redirect_uri.clone())
        .map_err(|e| ThaiRagError::Config(format!("Invalid redirect URI: {e}")))?;

    let http_client = reqwest::Client::new();

    let metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
        .await
        .map_err(|e| ThaiRagError::Internal(format!("OIDC discovery failed: {e}")))?;

    let client = openidconnect::core::CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(config.client_secret.clone())),
    )
    .set_redirect_uri(redirect_url);

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut auth_req = client.authorize_url(
        AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );

    for scope in &config.scopes {
        if scope != "openid" {
            auth_req = auth_req.add_scope(Scope::new(scope.clone()));
        }
    }

    let (authorize_url, csrf_state, nonce) = auth_req.set_pkce_challenge(pkce_challenge).url();

    Ok(AuthorizeResult {
        authorize_url: authorize_url.to_string(),
        state: csrf_state.secret().clone(),
        nonce: nonce.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
    })
}

// ── Exchange Code for Tokens + Extract User Info ────────────────────

/// Perform OIDC discovery, exchange authorization code for tokens, verify the
/// id_token, and extract user info.
pub async fn exchange_code_for_user(
    config: &OidcProviderConfig,
    code: &str,
    pkce_verifier: &str,
    nonce: &str,
) -> Result<OidcUserInfo> {
    let issuer_url = IssuerUrl::new(config.issuer_url.clone())
        .map_err(|e| ThaiRagError::Config(format!("Invalid issuer URL: {e}")))?;
    let redirect_url = RedirectUrl::new(config.redirect_uri.clone())
        .map_err(|e| ThaiRagError::Config(format!("Invalid redirect URI: {e}")))?;

    let http_client = reqwest::Client::new();

    let metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
        .await
        .map_err(|e| ThaiRagError::Internal(format!("OIDC discovery failed: {e}")))?;

    let client = openidconnect::core::CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(config.client_id.clone()),
        Some(ClientSecret::new(config.client_secret.clone())),
    )
    .set_redirect_uri(redirect_url);

    let token_response: CoreTokenResponse = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .map_err(|e| ThaiRagError::Auth(format!("Token exchange config error: {e}")))?
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|e| ThaiRagError::Auth(format!("Token exchange failed: {e}")))?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| ThaiRagError::Auth("No id_token in response".into()))?;

    let nonce_val = Nonce::new(nonce.to_string());
    let claims = id_token
        .claims(&client.id_token_verifier(), &nonce_val)
        .map_err(|e| ThaiRagError::Auth(format!("ID token verification failed: {e}")))?;

    let sub = claims.subject().to_string();

    let email: String = claims
        .email()
        .map(|e| e.as_str().to_string())
        .ok_or_else(|| ThaiRagError::Auth("No email claim in id_token".into()))?;

    let name: String = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| email.clone());

    // Extract realm_access.roles from the access_token's JWT payload.
    // Keycloak includes realm_access.roles in the access_token by default,
    // NOT in the id_token. We decode the access_token JWT payload to get them.
    let roles: Vec<String> = {
        let token_str = token_response.access_token().secret().to_string();
        let parts: Vec<&str> = token_str.split('.').collect();
        if parts.len() >= 2 {
            use base64::Engine;
            let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
            engine
                .decode(parts[1])
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|payload| {
                    payload
                        .get("realm_access")
                        .and_then(|ra| ra.get("roles"))
                        .and_then(|r| serde_json::from_value::<Vec<String>>(r.clone()).ok())
                })
                .unwrap_or_default()
        } else {
            vec![]
        }
    };

    Ok(OidcUserInfo {
        external_id: sub,
        email,
        name,
        roles,
    })
}

/// Resolve the highest ThaiRAG role from Keycloak roles using the mapping.
/// Priority: super_admin > admin > editor > viewer.
pub fn resolve_role(
    keycloak_roles: &[String],
    role_mapping: &std::collections::HashMap<String, String>,
) -> String {
    let role_priority = |r: &str| -> u8 {
        match r {
            "super_admin" => 4,
            "admin" => 3,
            "editor" => 2,
            "viewer" => 1,
            _ => 0,
        }
    };

    keycloak_roles
        .iter()
        .filter_map(|kc_role| role_mapping.get(kc_role))
        .max_by_key(|r| role_priority(r.as_str()))
        .cloned()
        .unwrap_or_else(|| "viewer".to_string())
}
