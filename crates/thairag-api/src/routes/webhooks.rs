use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::{WebhookEvent, WebhookId};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub secret: String,
    pub events: Vec<WebhookEvent>,
}

#[derive(Serialize)]
pub struct WebhookResponse {
    pub id: String,
    pub url: String,
    pub events: Vec<WebhookEvent>,
    pub is_active: bool,
    pub created_at: String,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn require_super_admin(claims: &AuthClaims, state: &AppState) -> Result<(), ApiError> {
    if claims.sub == "anonymous" {
        return Ok(());
    }
    let user_id = claims
        .sub
        .parse::<Uuid>()
        .map(thairag_core::types::UserId)
        .map_err(|_| ThaiRagError::Auth("Invalid user ID".into()))?;
    let user = state
        .km_store
        .get_user(user_id)
        .map_err(|_| ThaiRagError::Authorization("User not found".into()))?;
    if user.is_super_admin || user.role == "super_admin" {
        Ok(())
    } else {
        Err(ThaiRagError::Authorization("Only super admins can manage webhooks".into()).into())
    }
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /api/admin/webhooks
pub async fn create_webhook(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<WebhookResponse>), ApiError> {
    require_super_admin(&claims, &state)?;

    if body.url.trim().is_empty() {
        return Err(ThaiRagError::Validation("url must not be empty".into()).into());
    }
    if body.secret.trim().is_empty() {
        return Err(ThaiRagError::Validation("secret must not be empty".into()).into());
    }
    if body.events.is_empty() {
        return Err(ThaiRagError::Validation("events must not be empty".into()).into());
    }

    // Validate URL format
    if !body.url.starts_with("http://") && !body.url.starts_with("https://") {
        return Err(
            ThaiRagError::Validation("url must start with http:// or https://".into()).into(),
        );
    }

    let webhook = state
        .webhook_dispatcher
        .create_webhook(body.url, body.secret, body.events);

    Ok((
        StatusCode::CREATED,
        Json(WebhookResponse {
            id: webhook.id.to_string(),
            url: webhook.url,
            events: webhook.events,
            is_active: webhook.is_active,
            created_at: webhook.created_at,
        }),
    ))
}

/// GET /api/admin/webhooks
pub async fn list_webhooks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<WebhookResponse>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let webhooks = state.webhook_dispatcher.list_webhooks();
    let response: Vec<WebhookResponse> = webhooks
        .into_iter()
        .map(|w| WebhookResponse {
            id: w.id.to_string(),
            url: w.url,
            events: w.events,
            is_active: w.is_active,
            created_at: w.created_at,
        })
        .collect();

    Ok(Json(response))
}

/// DELETE /api/admin/webhooks/:webhook_id
pub async fn delete_webhook(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(webhook_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;

    let deleted = state
        .webhook_dispatcher
        .delete_webhook(WebhookId(webhook_id));
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ThaiRagError::NotFound("Webhook not found".into()).into())
    }
}

/// POST /api/admin/webhooks/:webhook_id/test
pub async fn test_webhook(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(webhook_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_super_admin(&claims, &state)?;

    let webhook = state
        .webhook_dispatcher
        .get_webhook(WebhookId(webhook_id))
        .ok_or_else(|| ThaiRagError::NotFound("Webhook not found".into()))?;

    // Fire a test event
    let test_data = serde_json::json!({
        "message": "This is a test webhook event from ThaiRAG",
        "webhook_id": webhook.id.to_string(),
    });

    state
        .webhook_dispatcher
        .dispatch(WebhookEvent::JobCompleted, test_data);

    Ok(Json(serde_json::json!({
        "status": "sent",
        "message": "Test event dispatched to all matching webhooks"
    })))
}
