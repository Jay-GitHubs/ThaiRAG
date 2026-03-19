use sha2::{Digest, Sha256};
use thairag_core::error::{Result, ThaiRagError};
use thairag_core::traits::McpClient;
use thairag_core::types::{
    ConnectorId, ConnectorStatus, DocId, McpConnectorConfig, McpResource, SyncRun, SyncRunId,
    SyncRunStatus, SyncState,
};
use tracing::{debug, info, warn};

/// Trait for the store operations the sync engine needs.
/// Implemented by the KmStore in `thairag-api`.
#[async_trait::async_trait]
pub trait SyncStore: Send + Sync {
    fn get_sync_state(&self, connector_id: ConnectorId, resource_uri: &str) -> Option<SyncState>;
    fn upsert_sync_state(&self, state: SyncState) -> Result<()>;
    fn insert_sync_run(&self, run: SyncRun) -> Result<()>;
    fn update_sync_run(&self, run: SyncRun) -> Result<()>;
    fn update_connector_status(&self, id: ConnectorId, status: ConnectorStatus) -> Result<()>;
}

/// Callback to ingest a single piece of content into the document pipeline.
/// Returns the DocId of the created/updated document.
#[async_trait::async_trait]
pub trait ContentIngester: Send + Sync {
    async fn ingest(
        &self,
        workspace_id: thairag_core::types::WorkspaceId,
        title: &str,
        content: &[u8],
        mime_type: &str,
        existing_doc_id: Option<DocId>,
    ) -> Result<DocId>;
}

/// Orchestrates a single sync run: connect → list → fetch → ingest → track.
pub struct SyncEngine {
    max_resource_size: usize,
    retry_max_attempts: u32,
    retry_base_delay: std::time::Duration,
    retry_max_delay: std::time::Duration,
}

impl SyncEngine {
    pub fn new(
        max_resource_size: usize,
        retry_max_attempts: u32,
        retry_base_delay_secs: u64,
        retry_max_delay_secs: u64,
    ) -> Self {
        Self {
            max_resource_size,
            retry_max_attempts,
            retry_base_delay: std::time::Duration::from_secs(retry_base_delay_secs),
            retry_max_delay: std::time::Duration::from_secs(retry_max_delay_secs),
        }
    }

    /// Execute a full sync run for a connector.
    pub async fn run_sync(
        &self,
        config: &McpConnectorConfig,
        client: &mut dyn McpClient,
        store: &dyn SyncStore,
        ingester: &dyn ContentIngester,
    ) -> Result<SyncRun> {
        let mut run = SyncRun {
            id: SyncRunId::new(),
            connector_id: config.id,
            started_at: chrono::Utc::now(),
            completed_at: None,
            status: SyncRunStatus::Running,
            items_discovered: 0,
            items_created: 0,
            items_updated: 0,
            items_skipped: 0,
            items_failed: 0,
            error_message: None,
        };

        store.insert_sync_run(run.clone())?;
        store.update_connector_status(config.id, ConnectorStatus::Syncing)?;

        info!(connector = %config.name, run_id = %run.id, "Starting sync run");

        // Connect + discover with retry
        let mut resources = Vec::new();
        let mut last_error = String::new();
        let mut connected = false;

        for attempt in 0..self.retry_max_attempts {
            if attempt > 0 {
                let delay = std::cmp::min(
                    self.retry_base_delay * 2u32.pow(attempt - 1),
                    self.retry_max_delay,
                );
                warn!(
                    connector = %config.name,
                    attempt = attempt + 1,
                    delay_secs = delay.as_secs(),
                    "Retrying sync after failure"
                );
                tokio::time::sleep(delay).await;
            }

            // Connect
            if let Err(e) = client.connect().await {
                last_error = format!("Connection failed: {e}");
                warn!(connector = %config.name, error = %e, attempt = attempt + 1, "Connect failed");
                continue;
            }
            connected = true;

            // Discover resources
            match self.discover_resources(config, client).await {
                Ok(r) => {
                    resources = r;
                    last_error.clear();
                    break;
                }
                Err(e) => {
                    last_error = format!("Resource discovery failed: {e}");
                    warn!(connector = %config.name, error = %e, attempt = attempt + 1, "Discovery failed");
                    let _ = client.disconnect().await;
                    connected = false;
                }
            }
        }

        if !last_error.is_empty() {
            run.status = SyncRunStatus::Failed;
            run.error_message = Some(last_error);
            run.completed_at = Some(chrono::Utc::now());
            store.update_sync_run(run.clone())?;
            store.update_connector_status(config.id, ConnectorStatus::Error)?;
            if connected {
                let _ = client.disconnect().await;
            }
            return Ok(run);
        }

        run.items_discovered = resources.len();
        info!(connector = %config.name, items = resources.len(), "Resources discovered");

        // Apply max_items_per_sync limit
        let limit = config.max_items_per_sync.unwrap_or(usize::MAX);
        let resources: Vec<_> = resources.into_iter().take(limit).collect();

        // Process each resource
        for resource in &resources {
            match self
                .process_resource(config, client, store, ingester, resource)
                .await
            {
                Ok(action) => match action {
                    SyncAction::Created => run.items_created += 1,
                    SyncAction::Updated => run.items_updated += 1,
                    SyncAction::Skipped => run.items_skipped += 1,
                },
                Err(e) => {
                    warn!(
                        connector = %config.name,
                        resource = %resource.uri,
                        error = %e,
                        "Failed to sync resource"
                    );
                    run.items_failed += 1;
                }
            }
        }

        // Finalize
        let _ = client.disconnect().await;
        run.status = SyncRunStatus::Completed;
        run.completed_at = Some(chrono::Utc::now());
        store.update_sync_run(run.clone())?;
        store.update_connector_status(config.id, ConnectorStatus::Active)?;

        info!(
            connector = %config.name,
            created = run.items_created,
            updated = run.items_updated,
            skipped = run.items_skipped,
            failed = run.items_failed,
            "Sync run completed"
        );

        Ok(run)
    }

    async fn discover_resources(
        &self,
        config: &McpConnectorConfig,
        client: &dyn McpClient,
    ) -> Result<Vec<McpResource>> {
        let mut resources = Vec::new();

        // List resources from the MCP server
        match client.list_resources().await {
            Ok(listed) => {
                // Apply resource_filters if configured
                let filtered = if config.resource_filters.is_empty() {
                    listed
                } else {
                    listed
                        .into_iter()
                        .filter(|r| {
                            config
                                .resource_filters
                                .iter()
                                .any(|f| matches_glob(f, &r.uri))
                        })
                        .collect()
                };
                resources.extend(filtered);
            }
            Err(e) => {
                debug!(
                    connector = %config.name,
                    error = %e,
                    "list_resources not supported, trying tool-based discovery"
                );
            }
        }

        // Execute pre-configured tool calls for tool-based sources
        for tc in &config.tool_calls {
            match client.call_tool(&tc.tool_name, tc.arguments.clone()).await {
                Ok(result) => {
                    // Each result item becomes a synthetic resource
                    if let serde_json::Value::Array(items) = &result {
                        for (i, _item) in items.iter().enumerate() {
                            let title = tc
                                .title_template
                                .replace("{index}", &i.to_string())
                                .replace(
                                    "{date}",
                                    &chrono::Utc::now().format("%Y-%m-%d").to_string(),
                                );
                            let uri = format!("tool://{}?index={}", tc.tool_name, i);
                            resources.push(McpResource {
                                uri,
                                name: if title.is_empty() {
                                    format!("{} #{}", tc.tool_name, i)
                                } else {
                                    title
                                },
                                mime_type: Some(tc.result_mime_type.clone()),
                                description: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        connector = %config.name,
                        tool = %tc.tool_name,
                        error = %e,
                        "Tool call failed during discovery"
                    );
                }
            }
        }

        Ok(resources)
    }

    async fn process_resource(
        &self,
        config: &McpConnectorConfig,
        client: &dyn McpClient,
        store: &dyn SyncStore,
        ingester: &dyn ContentIngester,
        resource: &McpResource,
    ) -> Result<SyncAction> {
        debug!(resource = %resource.uri, "Processing resource");

        // Read resource content
        let content = client.read_resource(&resource.uri).await?;

        // Check size limit
        if content.data.len() > self.max_resource_size {
            return Err(ThaiRagError::Internal(format!(
                "Resource {} exceeds max size ({} > {} bytes)",
                resource.uri,
                content.data.len(),
                self.max_resource_size
            )));
        }

        // Compute content hash for change detection
        let hash = compute_hash(&content.data);

        // Check existing sync state
        let existing = store.get_sync_state(config.id, &resource.uri);

        if let Some(ref state) = existing
            && state.content_hash == hash
        {
            debug!(resource = %resource.uri, "Content unchanged, skipping");
            return Ok(SyncAction::Skipped);
        }

        // Ingest into document pipeline
        let existing_doc_id = existing.as_ref().and_then(|s| s.doc_id);
        let doc_id = ingester
            .ingest(
                config.workspace_id,
                &resource.name,
                &content.data,
                &content.mime_type,
                existing_doc_id,
            )
            .await?;

        // Update sync state
        let sync_state = SyncState {
            connector_id: config.id,
            resource_uri: resource.uri.clone(),
            content_hash: hash,
            doc_id: Some(doc_id),
            last_synced_at: chrono::Utc::now(),
            source_metadata: None,
        };
        store.upsert_sync_state(sync_state)?;

        if existing.is_some() {
            Ok(SyncAction::Updated)
        } else {
            Ok(SyncAction::Created)
        }
    }
}

enum SyncAction {
    Created,
    Updated,
    Skipped,
}

fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Simple glob matching for resource URI filters.
/// Supports `*` as wildcard for any sequence of non-`/` chars
/// and `**` for any sequence including `/`.
fn matches_glob(pattern: &str, text: &str) -> bool {
    let regex_str = pattern
        .replace("**", "\x00")
        .replace('*', "[^/]*")
        .replace('\x00', ".*");
    regex_str == text || text.contains(&regex_str) || text.starts_with(&pattern.replace('*', ""))
}
