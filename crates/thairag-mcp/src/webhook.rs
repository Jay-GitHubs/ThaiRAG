use serde::Serialize;
use tracing::{info, warn};

#[derive(Serialize)]
pub struct WebhookPayload {
    pub event: String,
    pub connector_id: String,
    pub connector_name: String,
    pub items_created: usize,
    pub items_updated: usize,
    pub items_skipped: usize,
    pub items_failed: usize,
    pub error_message: Option<String>,
    pub timestamp: String,
}

/// Send a webhook notification. Failures are logged but never propagated.
pub async fn send_webhook(url: &str, secret: Option<&str>, payload: &WebhookPayload) {
    let body = match serde_json::to_vec(payload) {
        Ok(b) => b,
        Err(e) => {
            warn!(url, error = %e, "Failed to serialize webhook payload");
            return;
        }
    };

    let client = reqwest::Client::new();
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(10));

    // Auth via bearer token if secret is configured
    if let Some(secret) = secret {
        request = request.header("Authorization", format!("Bearer {secret}"));
    }

    match request.body(body).send().await {
        Ok(resp) => {
            info!(url, status = %resp.status(), "Webhook sent");
        }
        Err(e) => {
            warn!(url, error = %e, "Webhook delivery failed");
        }
    }
}
