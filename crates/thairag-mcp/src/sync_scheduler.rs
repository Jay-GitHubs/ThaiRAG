use std::collections::HashMap;
use std::sync::Arc;

use thairag_core::types::{ConnectorId, McpConnectorConfig, SyncMode, SyncRunStatus};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::sync_engine::{ContentIngester, SyncEngine, SyncStore};

/// Callback invoked after each sync run completes (for metrics, notifications, etc.).
pub type SyncRunCallback = Arc<
    dyn Fn(
            &str, // connector_name
            &str, // status ("completed" | "failed")
            f64,  // duration_secs
            u64,  // items_created
            u64,  // items_updated
            u64,  // items_skipped
            u64,  // items_failed
        ) + Send
        + Sync,
>;

/// Manages background sync tasks for all scheduled connectors.
pub struct SyncScheduler {
    tasks: RwLock<HashMap<ConnectorId, ScheduledTask>>,
    cancel_token: CancellationToken,
    engine: Arc<SyncEngine>,
    store: Arc<dyn SyncStore>,
    ingester: Arc<dyn ContentIngester>,
    on_sync_complete: Option<SyncRunCallback>,
}

struct ScheduledTask {
    handle: JoinHandle<()>,
    cancel: CancellationToken,
}

impl SyncScheduler {
    pub fn new(
        engine: Arc<SyncEngine>,
        store: Arc<dyn SyncStore>,
        ingester: Arc<dyn ContentIngester>,
    ) -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            cancel_token: CancellationToken::new(),
            engine,
            store,
            ingester,
            on_sync_complete: None,
        }
    }

    /// Set a callback invoked after each scheduled sync run (for metrics/notifications).
    pub fn with_on_sync_complete(mut self, callback: SyncRunCallback) -> Self {
        self.on_sync_complete = Some(callback);
        self
    }

    /// Start scheduled tasks for all active connectors with SyncMode::Scheduled.
    pub async fn start(&self, connectors: Vec<McpConnectorConfig>) {
        for config in connectors {
            if config.sync_mode == SyncMode::Scheduled {
                self.add_connector(config).await;
            }
        }
        let task_count = self.tasks.read().await.len();
        info!(tasks = task_count, "Sync scheduler started");
    }

    /// Add or replace a scheduled connector.
    pub async fn add_connector(&self, config: McpConnectorConfig) {
        let cron_expr = match &config.schedule_cron {
            Some(c) => c.clone(),
            None => {
                warn!(
                    connector = %config.name,
                    "Scheduled connector missing cron expression, skipping"
                );
                return;
            }
        };

        // Parse cron
        let schedule = match cron_expr.parse::<cron::Schedule>() {
            Ok(s) => s,
            Err(e) => {
                error!(
                    connector = %config.name,
                    error = %e,
                    "Invalid cron expression: {cron_expr}"
                );
                return;
            }
        };

        // Remove existing task if any
        self.remove_connector(config.id).await;

        let task_cancel = self.cancel_token.child_token();
        let engine = Arc::clone(&self.engine);
        let store = Arc::clone(&self.store);
        let ingester = Arc::clone(&self.ingester);
        let connector_id = config.id;
        let connector_name = config.name.clone();
        let token = task_cancel.clone();
        let on_complete = self.on_sync_complete.clone();

        let handle = tokio::spawn(async move {
            info!(
                connector = %connector_name,
                cron = %cron_expr,
                "Scheduled sync task started"
            );

            loop {
                // Compute next run time
                let now = chrono::Utc::now();
                let next = match schedule.upcoming(chrono::Utc).next() {
                    Some(t) => t,
                    None => {
                        warn!(connector = %connector_name, "No upcoming schedule, stopping");
                        break;
                    }
                };

                let delay = (next - now)
                    .to_std()
                    .unwrap_or(std::time::Duration::from_secs(60));

                // Wait until next run or cancellation
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = token.cancelled() => {
                        info!(connector = %connector_name, "Scheduled sync task cancelled");
                        break;
                    }
                }

                if token.is_cancelled() {
                    break;
                }

                info!(connector = %connector_name, "Running scheduled sync");

                let start = std::time::Instant::now();

                // Create a new MCP client for each run
                let mut client = crate::client::RmcpClient::new(
                    config.clone(),
                    std::time::Duration::from_secs(30),
                    std::time::Duration::from_secs(120),
                );

                match engine
                    .run_sync(&config, &mut client, store.as_ref(), ingester.as_ref())
                    .await
                {
                    Ok(run) => {
                        let status_str = match run.status {
                            SyncRunStatus::Completed => "completed",
                            SyncRunStatus::Failed => "failed",
                            _ => "other",
                        };
                        info!(
                            connector = %connector_name,
                            status = status_str,
                            created = run.items_created,
                            updated = run.items_updated,
                            skipped = run.items_skipped,
                            failed = run.items_failed,
                            "Scheduled sync completed"
                        );
                        if let Some(ref cb) = on_complete {
                            cb(
                                &connector_name,
                                status_str,
                                start.elapsed().as_secs_f64(),
                                run.items_created as u64,
                                run.items_updated as u64,
                                run.items_skipped as u64,
                                run.items_failed as u64,
                            );
                        }
                        // Fire webhook if configured
                        if let Some(ref webhook_url) = config.webhook_url {
                            let payload = crate::webhook::WebhookPayload {
                                event: if matches!(run.status, SyncRunStatus::Completed) {
                                    "sync.completed"
                                } else {
                                    "sync.failed"
                                }
                                .to_string(),
                                connector_id: config.id.to_string(),
                                connector_name: connector_name.clone(),
                                items_created: run.items_created,
                                items_updated: run.items_updated,
                                items_skipped: run.items_skipped,
                                items_failed: run.items_failed,
                                error_message: run.error_message.clone(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            };
                            crate::webhook::send_webhook(
                                webhook_url,
                                config.webhook_secret.as_deref(),
                                &payload,
                            )
                            .await;
                        }
                    }
                    Err(e) => {
                        error!(
                            connector = %connector_name,
                            error = %e,
                            "Scheduled sync failed"
                        );
                        if let Some(ref cb) = on_complete {
                            cb(
                                &connector_name,
                                "failed",
                                start.elapsed().as_secs_f64(),
                                0,
                                0,
                                0,
                                0,
                            );
                        }
                        // Fire webhook on error if configured
                        if let Some(ref webhook_url) = config.webhook_url {
                            let payload = crate::webhook::WebhookPayload {
                                event: "sync.failed".to_string(),
                                connector_id: config.id.to_string(),
                                connector_name: connector_name.clone(),
                                items_created: 0,
                                items_updated: 0,
                                items_skipped: 0,
                                items_failed: 0,
                                error_message: Some(e.to_string()),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            };
                            crate::webhook::send_webhook(
                                webhook_url,
                                config.webhook_secret.as_deref(),
                                &payload,
                            )
                            .await;
                        }
                    }
                }
            }
        });

        self.tasks.write().await.insert(
            connector_id,
            ScheduledTask {
                handle,
                cancel: task_cancel,
            },
        );
    }

    /// Remove a scheduled connector.
    pub async fn remove_connector(&self, id: ConnectorId) {
        if let Some(task) = self.tasks.write().await.remove(&id) {
            task.cancel.cancel();
            let _ = task.handle.await;
        }
    }

    /// Gracefully shut down all scheduled tasks.
    pub async fn shutdown(&self) {
        info!("Shutting down sync scheduler");
        self.cancel_token.cancel();
        let mut tasks = self.tasks.write().await;
        for (_, task) in tasks.drain() {
            let _ = task.handle.await;
        }
    }
}
