use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use thairag_core::types::{Webhook, WebhookEvent, WebhookId, WebhookPayload};
use tracing::{info, warn};

use crate::store::KmStoreTrait;

type HmacSha256 = Hmac<Sha256>;

/// Settings key prefix for webhook storage.
const WEBHOOK_KEY_PREFIX: &str = "_webhook.";

/// Manages webhook registration and dispatching.
#[derive(Clone)]
pub struct WebhookDispatcher {
    client: reqwest::Client,
    km_store: Arc<dyn KmStoreTrait>,
}

impl WebhookDispatcher {
    pub fn new(km_store: Arc<dyn KmStoreTrait>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { client, km_store }
    }

    /// Register a new webhook.
    pub fn create_webhook(
        &self,
        url: String,
        secret: String,
        events: Vec<WebhookEvent>,
    ) -> Webhook {
        let webhook = Webhook {
            id: WebhookId::new(),
            url,
            secret,
            events,
            is_active: true,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let json = serde_json::to_string(&webhook).unwrap_or_default();
        self.km_store
            .set_setting(&format!("{WEBHOOK_KEY_PREFIX}{}", webhook.id), &json);
        // Track webhook IDs in an index key
        self.add_to_index(webhook.id);
        webhook
    }

    /// List all registered webhooks.
    pub fn list_webhooks(&self) -> Vec<Webhook> {
        self.webhook_ids()
            .into_iter()
            .filter_map(|id| {
                let json = self
                    .km_store
                    .get_setting(&format!("{WEBHOOK_KEY_PREFIX}{id}"))?;
                serde_json::from_str::<Webhook>(&json).ok()
            })
            .collect()
    }

    /// Delete a webhook by ID.
    pub fn delete_webhook(&self, id: WebhookId) -> bool {
        let key = format!("{WEBHOOK_KEY_PREFIX}{id}");
        if self.km_store.get_setting(&key).is_some() {
            self.km_store.delete_setting(&key);
            self.remove_from_index(id);
            true
        } else {
            false
        }
    }

    /// Get a webhook by ID.
    pub fn get_webhook(&self, id: WebhookId) -> Option<Webhook> {
        let json = self
            .km_store
            .get_setting(&format!("{WEBHOOK_KEY_PREFIX}{id}"))?;
        serde_json::from_str(&json).ok()
    }

    /// Dispatch an event to all matching webhooks (fire-and-forget).
    pub fn dispatch(&self, event: WebhookEvent, data: serde_json::Value) {
        let webhooks = self.get_webhooks_for_event(&event);
        if webhooks.is_empty() {
            return;
        }

        let payload = WebhookPayload {
            event,
            timestamp: chrono::Utc::now().to_rfc3339(),
            data,
        };

        let payload_json = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "Failed to serialize webhook payload");
                return;
            }
        };

        for webhook in webhooks {
            let client = self.client.clone();
            let body = payload_json.clone();
            let url = webhook.url.clone();
            let secret = webhook.secret.clone();

            tokio::spawn(async move {
                let signature = compute_signature(&secret, &body);
                let result = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("X-ThaiRAG-Signature", signature)
                    .body(body)
                    .send()
                    .await;

                match result {
                    Ok(resp) => {
                        info!(url, status = %resp.status(), "Webhook delivered");
                    }
                    Err(e) => {
                        warn!(url, error = %e, "Webhook delivery failed");
                    }
                }
            });
        }
    }

    /// Get all active webhooks subscribed to a given event.
    fn get_webhooks_for_event(&self, event: &WebhookEvent) -> Vec<Webhook> {
        self.list_webhooks()
            .into_iter()
            .filter(|w| w.is_active && w.events.contains(event))
            .collect()
    }

    // ── Index management (stored as comma-separated IDs) ──

    fn index_key() -> String {
        format!("{WEBHOOK_KEY_PREFIX}_index")
    }

    fn webhook_ids(&self) -> Vec<String> {
        self.km_store
            .get_setting(&Self::index_key())
            .map(|s| {
                s.split(',')
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn add_to_index(&self, id: WebhookId) {
        let mut ids = self.webhook_ids();
        let id_str = id.to_string();
        if !ids.contains(&id_str) {
            ids.push(id_str);
            self.km_store
                .set_setting(&Self::index_key(), &ids.join(","));
        }
    }

    fn remove_from_index(&self, id: WebhookId) {
        let ids: Vec<String> = self
            .webhook_ids()
            .into_iter()
            .filter(|i| i != &id.to_string())
            .collect();
        if ids.is_empty() {
            self.km_store.delete_setting(&Self::index_key());
        } else {
            self.km_store
                .set_setting(&Self::index_key(), &ids.join(","));
        }
    }
}

/// Compute HMAC-SHA256 signature for a payload.
fn compute_signature(secret: &str, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(body);
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}
