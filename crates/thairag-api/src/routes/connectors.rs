use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::{
    ConnectorId, ConnectorStatus, McpConnectorConfig, McpTransport, SyncMode, SyncRun, SyncState,
    ToolCallConfig, WorkspaceId,
};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};

use super::km::{ListResponse, PaginationParams, paginate};

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateConnectorRequest {
    pub name: String,
    pub description: Option<String>,
    pub transport: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub workspace_id: Uuid,
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
    pub schedule_cron: Option<String>,
    #[serde(default)]
    pub resource_filters: Vec<String>,
    pub max_items_per_sync: Option<usize>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallConfig>,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
}

fn default_sync_mode() -> String {
    "on_demand".into()
}

#[derive(Deserialize)]
pub struct UpdateConnectorRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub transport: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub sync_mode: Option<String>,
    pub schedule_cron: Option<String>,
    pub resource_filters: Option<Vec<String>>,
    pub max_items_per_sync: Option<usize>,
    pub tool_calls: Option<Vec<ToolCallConfig>>,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
}

#[derive(Serialize)]
pub struct ConnectorResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub workspace_id: String,
    pub sync_mode: String,
    pub schedule_cron: Option<String>,
    pub resource_filters: Vec<String>,
    pub max_items_per_sync: Option<usize>,
    pub tool_calls: Vec<ToolCallConfig>,
    pub webhook_url: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_sync_at: Option<String>,
    pub last_sync_status: Option<String>,
}

impl From<McpConnectorConfig> for ConnectorResponse {
    fn from(c: McpConnectorConfig) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name,
            description: c.description,
            transport: format!("{:?}", c.transport).to_lowercase(),
            command: c.command,
            args: c.args,
            url: c.url,
            workspace_id: c.workspace_id.to_string(),
            sync_mode: serde_json::to_value(&c.sync_mode)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            schedule_cron: c.schedule_cron,
            resource_filters: c.resource_filters,
            max_items_per_sync: c.max_items_per_sync,
            tool_calls: c.tool_calls,
            webhook_url: c.webhook_url,
            status: serde_json::to_value(&c.status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
            last_sync_at: None,
            last_sync_status: None,
        }
    }
}

#[derive(Serialize)]
pub struct SyncRunResponse {
    pub id: String,
    pub connector_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub items_discovered: usize,
    pub items_created: usize,
    pub items_updated: usize,
    pub items_skipped: usize,
    pub items_failed: usize,
    pub error_message: Option<String>,
    pub duration_secs: Option<f64>,
}

impl From<SyncRun> for SyncRunResponse {
    fn from(r: SyncRun) -> Self {
        Self {
            id: r.id.to_string(),
            connector_id: r.connector_id.to_string(),
            started_at: r.started_at.to_rfc3339(),
            completed_at: r.completed_at.map(|t| t.to_rfc3339()),
            status: serde_json::to_value(&r.status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            items_discovered: r.items_discovered,
            items_created: r.items_created,
            items_updated: r.items_updated,
            items_skipped: r.items_skipped,
            items_failed: r.items_failed,
            error_message: r.error_message,
            duration_secs: r
                .completed_at
                .map(|end| (end - r.started_at).num_milliseconds() as f64 / 1000.0),
        }
    }
}

#[derive(Serialize)]
pub struct ResourceListResponse {
    pub resources: Vec<thairag_core::types::McpResource>,
}

// ── Templates ────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ConnectorTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env_keys: Vec<String>,
    pub url: Option<String>,
    pub resource_filters: Vec<String>,
}

fn connector_templates() -> Vec<ConnectorTemplate> {
    vec![
        ConnectorTemplate {
            id: "filesystem".into(),
            name: "Filesystem".into(),
            description: "Read files from a local directory via MCP filesystem server".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
                "/data".into(),
            ],
            env_keys: vec![],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "fetch".into(),
            name: "Web Fetch".into(),
            description: "Fetch and extract content from web URLs".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-fetch".into()],
            env_keys: vec![],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "postgres".into(),
            name: "PostgreSQL".into(),
            description: "Connect to PostgreSQL database and sync table data".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-postgres".into()],
            env_keys: vec!["POSTGRES_URL".into()],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "sqlite".into(),
            name: "SQLite".into(),
            description: "Connect to a SQLite database file".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-sqlite".into()],
            env_keys: vec!["SQLITE_PATH".into()],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "github".into(),
            name: "GitHub".into(),
            description: "Access GitHub repositories, issues, and pull requests".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            env_keys: vec!["GITHUB_TOKEN".into()],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "slack".into(),
            name: "Slack".into(),
            description: "Access Slack channels, messages, and threads".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-slack".into()],
            env_keys: vec!["SLACK_BOT_TOKEN".into()],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "google-drive".into(),
            name: "Google Drive".into(),
            description: "Sync documents from Google Drive".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-gdrive".into()],
            env_keys: vec!["GOOGLE_APPLICATION_CREDENTIALS".into()],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "notion".into(),
            name: "Notion".into(),
            description: "Sync pages and databases from Notion".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@notionhq/notion-mcp-server".into()],
            env_keys: vec!["NOTION_API_KEY".into()],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "confluence".into(),
            name: "Confluence".into(),
            description: "Sync pages from Atlassian Confluence".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@aashari/server-atlassian-confluence".into()],
            env_keys: vec![
                "CONFLUENCE_BASE_URL".into(),
                "CONFLUENCE_USER_EMAIL".into(),
                "CONFLUENCE_API_TOKEN".into(),
            ],
            url: None,
            resource_filters: vec![],
        },
        ConnectorTemplate {
            id: "onedrive".into(),
            name: "Microsoft OneDrive".into(),
            description: "Sync documents from Microsoft OneDrive / SharePoint".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@anthropic/onedrive-mcp-server".into()],
            env_keys: vec![
                "MICROSOFT_CLIENT_ID".into(),
                "MICROSOFT_CLIENT_SECRET".into(),
                "MICROSOFT_TENANT_ID".into(),
            ],
            url: None,
            resource_filters: vec![],
        },
    ]
}

// ── Helpers ─────────────────────────────────────────────────────────

fn parse_transport(s: &str) -> Result<McpTransport, ApiError> {
    match s {
        "stdio" => Ok(McpTransport::Stdio),
        "sse" => Ok(McpTransport::Sse),
        _ => Err(ThaiRagError::Validation(format!(
            "Invalid transport: {s}. Must be 'stdio' or 'sse'"
        ))
        .into()),
    }
}

fn parse_sync_mode(s: &str) -> Result<SyncMode, ApiError> {
    match s {
        "on_demand" => Ok(SyncMode::OnDemand),
        "scheduled" => Ok(SyncMode::Scheduled),
        _ => Err(ThaiRagError::Validation(format!(
            "Invalid sync_mode: {s}. Must be 'on_demand' or 'scheduled'"
        ))
        .into()),
    }
}

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
        Err(ThaiRagError::Authorization("Only super admins can manage connectors".into()).into())
    }
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /api/km/connectors
pub async fn create_connector(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<CreateConnectorRequest>,
) -> Result<(StatusCode, Json<ConnectorResponse>), ApiError> {
    require_super_admin(&claims, &state)?;

    if body.name.trim().is_empty() {
        return Err(ThaiRagError::Validation("name must not be empty".into()).into());
    }

    let transport = parse_transport(&body.transport)?;
    let sync_mode = parse_sync_mode(&body.sync_mode)?;

    // Validate transport-specific fields
    match &transport {
        McpTransport::Stdio if body.command.is_none() => {
            return Err(
                ThaiRagError::Validation("stdio transport requires 'command'".into()).into(),
            );
        }
        McpTransport::Sse if body.url.is_none() => {
            return Err(ThaiRagError::Validation("sse transport requires 'url'".into()).into());
        }
        _ => {}
    }

    if sync_mode == SyncMode::Scheduled && body.schedule_cron.is_none() {
        return Err(
            ThaiRagError::Validation("scheduled sync requires 'schedule_cron'".into()).into(),
        );
    }

    let now = chrono::Utc::now();
    let config = McpConnectorConfig {
        id: ConnectorId::new(),
        name: body.name.trim().to_string(),
        description: body.description.unwrap_or_default(),
        transport,
        command: body.command,
        args: body.args,
        env: body.env,
        url: body.url,
        headers: body.headers,
        workspace_id: WorkspaceId(body.workspace_id),
        sync_mode,
        schedule_cron: body.schedule_cron,
        resource_filters: body.resource_filters,
        max_items_per_sync: body.max_items_per_sync,
        tool_calls: body.tool_calls,
        webhook_url: body.webhook_url,
        webhook_secret: body.webhook_secret,
        status: ConnectorStatus::Active,
        created_at: now,
        updated_at: now,
    };

    let created = state.km_store.insert_connector(config)?;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// GET /api/km/connectors
pub async fn list_connectors(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<ConnectorResponse>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let all = state.km_store.list_connectors();
    let (data, total) = paginate(all, &params);
    let data: Vec<ConnectorResponse> = data
        .into_iter()
        .map(|c| {
            let latest = state.km_store.get_latest_sync_run(c.id);
            let mut resp: ConnectorResponse = c.into();
            if let Some(run) = latest {
                resp.last_sync_at = Some(run.started_at.to_rfc3339());
                resp.last_sync_status = Some(
                    serde_json::to_value(&run.status)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_default(),
                );
            }
            resp
        })
        .collect();
    Ok(Json(ListResponse { data, total }))
}

/// GET /api/km/connectors/templates
pub async fn list_connector_templates() -> Json<Vec<ConnectorTemplate>> {
    Json(connector_templates())
}

#[derive(Deserialize)]
pub struct CreateFromTemplateRequest {
    pub template_id: String,
    pub workspace_id: Uuid,
    pub name: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
    pub schedule_cron: Option<String>,
}

/// POST /api/km/connectors/from-template
pub async fn create_from_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(body): AppJson<CreateFromTemplateRequest>,
) -> Result<(StatusCode, Json<ConnectorResponse>), ApiError> {
    require_super_admin(&claims, &state)?;

    let template = connector_templates()
        .into_iter()
        .find(|t| t.id == body.template_id)
        .ok_or_else(|| {
            ThaiRagError::NotFound(format!("Template '{}' not found", body.template_id))
        })?;

    // Validate required env keys
    for key in &template.env_keys {
        if !body.env.contains_key(key) {
            return Err(ThaiRagError::Validation(format!(
                "Missing required environment variable: {key}"
            ))
            .into());
        }
    }

    let transport = parse_transport(&template.transport)?;
    let sync_mode = parse_sync_mode(&body.sync_mode)?;

    if sync_mode == SyncMode::Scheduled && body.schedule_cron.is_none() {
        return Err(
            ThaiRagError::Validation("scheduled sync requires 'schedule_cron'".into()).into(),
        );
    }

    let now = chrono::Utc::now();
    let config = McpConnectorConfig {
        id: ConnectorId::new(),
        name: body.name.unwrap_or(template.name),
        description: template.description,
        transport,
        command: template.command,
        args: template.args,
        env: body.env,
        url: template.url,
        headers: HashMap::new(),
        workspace_id: WorkspaceId(body.workspace_id),
        sync_mode,
        schedule_cron: body.schedule_cron,
        resource_filters: template.resource_filters,
        max_items_per_sync: None,
        tool_calls: vec![],
        webhook_url: None,
        webhook_secret: None,
        status: ConnectorStatus::Active,
        created_at: now,
        updated_at: now,
    };

    let created = state.km_store.insert_connector(config)?;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// GET /api/km/connectors/:id
pub async fn get_connector(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<ConnectorResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let config = state.km_store.get_connector(ConnectorId(id))?;
    let latest_run = state.km_store.get_latest_sync_run(config.id);
    let mut resp: ConnectorResponse = config.into();
    if let Some(run) = latest_run {
        resp.last_sync_at = Some(run.started_at.to_rfc3339());
        resp.last_sync_status = Some(
            serde_json::to_value(&run.status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
        );
    }
    Ok(Json(resp))
}

/// PUT /api/km/connectors/:id
pub async fn update_connector(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    AppJson(body): AppJson<UpdateConnectorRequest>,
) -> Result<Json<ConnectorResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    let mut config = state.km_store.get_connector(ConnectorId(id))?;

    if let Some(name) = body.name {
        config.name = name;
    }
    if let Some(desc) = body.description {
        config.description = desc;
    }
    if let Some(transport) = body.transport {
        config.transport = parse_transport(&transport)?;
    }
    if body.command.is_some() {
        config.command = body.command;
    }
    if let Some(args) = body.args {
        config.args = args;
    }
    if let Some(env) = body.env {
        config.env = env;
    }
    if body.url.is_some() {
        config.url = body.url;
    }
    if let Some(headers) = body.headers {
        config.headers = headers;
    }
    if let Some(sync_mode) = body.sync_mode {
        config.sync_mode = parse_sync_mode(&sync_mode)?;
    }
    if body.schedule_cron.is_some() {
        config.schedule_cron = body.schedule_cron;
    }
    if let Some(filters) = body.resource_filters {
        config.resource_filters = filters;
    }
    if body.max_items_per_sync.is_some() {
        config.max_items_per_sync = body.max_items_per_sync;
    }
    if let Some(tc) = body.tool_calls {
        config.tool_calls = tc;
    }
    if body.webhook_url.is_some() {
        config.webhook_url = body.webhook_url;
    }
    if body.webhook_secret.is_some() {
        config.webhook_secret = body.webhook_secret;
    }

    config.updated_at = chrono::Utc::now();
    state.km_store.update_connector(config.clone())?;
    Ok(Json(config.into()))
}

/// DELETE /api/km/connectors/:id
pub async fn delete_connector(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;

    state.km_store.delete_connector(ConnectorId(id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/km/connectors/:id/sync
pub async fn trigger_sync(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<SyncRunResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    if !state.config.mcp.enabled {
        return Err(ThaiRagError::Validation("MCP is not enabled".into()).into());
    }

    let config = state.km_store.get_connector(ConnectorId(id))?;

    // Run sync asynchronously
    let km_store = state.km_store.clone();
    let mcp_config = state.config.mcp.clone();
    let sync_state = state.clone();

    let engine = thairag_mcp::SyncEngine::new(
        mcp_config.max_resource_size_bytes,
        mcp_config.sync_retry_max_attempts,
        mcp_config.sync_retry_base_delay_secs,
        mcp_config.sync_retry_max_delay_secs,
    );
    let mut client = thairag_mcp::RmcpClient::new(
        config.clone(),
        std::time::Duration::from_secs(mcp_config.connect_timeout_secs),
        std::time::Duration::from_secs(mcp_config.read_timeout_secs),
    );

    let store_adapter = StoreAdapter(km_store);
    let ingester = DocumentIngester { state: sync_state };

    let start = std::time::Instant::now();
    let connector_name = config.name.clone();

    let run = engine
        .run_sync(&config, &mut client, &store_adapter, &ingester)
        .await
        .map_err(|e| ThaiRagError::Internal(format!("Sync failed: {e}")))?;

    // Record metrics
    let status_str = match run.status {
        thairag_core::types::SyncRunStatus::Completed => "completed",
        thairag_core::types::SyncRunStatus::Failed => "failed",
        _ => "other",
    };
    state.metrics.record_sync_run(
        &connector_name,
        status_str,
        start.elapsed().as_secs_f64(),
        run.items_created as u64,
        run.items_updated as u64,
        run.items_skipped as u64,
        run.items_failed as u64,
    );

    // Fire webhook notification if configured
    if let Some(ref webhook_url) = config.webhook_url {
        let event = match run.status {
            thairag_core::types::SyncRunStatus::Completed => "sync.completed",
            thairag_core::types::SyncRunStatus::Failed => "sync.failed",
            _ => "sync.completed",
        };
        let payload = thairag_mcp::webhook::WebhookPayload {
            event: event.to_string(),
            connector_id: config.id.to_string(),
            connector_name: connector_name.clone(),
            items_created: run.items_created,
            items_updated: run.items_updated,
            items_skipped: run.items_skipped,
            items_failed: run.items_failed,
            error_message: run.error_message.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let webhook_url = webhook_url.clone();
        let webhook_secret = config.webhook_secret.clone();
        tokio::spawn(async move {
            thairag_mcp::webhook::send_webhook(&webhook_url, webhook_secret.as_deref(), &payload)
                .await;
        });
    }

    Ok(Json(run.into()))
}

/// POST /api/km/connectors/:id/pause
pub async fn pause_connector(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;

    state
        .km_store
        .update_connector_status(ConnectorId(id), ConnectorStatus::Paused)?;
    Ok(StatusCode::OK)
}

/// POST /api/km/connectors/:id/resume
pub async fn resume_connector(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;

    state
        .km_store
        .update_connector_status(ConnectorId(id), ConnectorStatus::Active)?;
    Ok(StatusCode::OK)
}

/// GET /api/km/connectors/:id/sync-runs
pub async fn list_sync_runs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<SyncRunResponse>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let runs = state.km_store.list_sync_runs(ConnectorId(id), params.limit);
    let total = runs.len();
    Ok(Json(ListResponse {
        data: runs.into_iter().map(Into::into).collect(),
        total,
    }))
}

/// POST /api/km/connectors/:id/test
pub async fn test_connection(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<ResourceListResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    if !state.config.mcp.enabled {
        return Err(ThaiRagError::Validation("MCP is not enabled".into()).into());
    }

    let config = state.km_store.get_connector(ConnectorId(id))?;
    let mcp_config = state.config.mcp.clone();

    use thairag_core::traits::McpClient;

    let mut client = thairag_mcp::RmcpClient::new(
        config,
        std::time::Duration::from_secs(mcp_config.connect_timeout_secs),
        std::time::Duration::from_secs(mcp_config.read_timeout_secs),
    );
    client
        .connect()
        .await
        .map_err(|e| ThaiRagError::Internal(format!("Connection failed: {e}")))?;
    let resources = client.list_resources().await;
    let _ = client.disconnect().await;
    let resources =
        resources.map_err(|e| ThaiRagError::Internal(format!("List resources failed: {e}")))?;

    Ok(Json(ResourceListResponse { resources }))
}

// ── Store adapter for SyncEngine ────────────────────────────────────

pub struct StoreAdapter(pub std::sync::Arc<dyn crate::store::KmStoreTrait>);

impl thairag_mcp::sync_engine::SyncStore for StoreAdapter {
    fn get_sync_state(&self, connector_id: ConnectorId, resource_uri: &str) -> Option<SyncState> {
        self.0.get_sync_state(connector_id, resource_uri)
    }

    fn upsert_sync_state(&self, state: SyncState) -> thairag_core::error::Result<()> {
        self.0.upsert_sync_state(state)
    }

    fn insert_sync_run(&self, run: SyncRun) -> thairag_core::error::Result<()> {
        self.0.insert_sync_run(run)
    }

    fn update_sync_run(&self, run: SyncRun) -> thairag_core::error::Result<()> {
        self.0.update_sync_run(run)
    }

    fn update_connector_status(
        &self,
        id: ConnectorId,
        status: ConnectorStatus,
    ) -> thairag_core::error::Result<()> {
        self.0.update_connector_status(id, status)
    }
}

/// Real ingester that wires MCP content into the DocumentPipeline + SearchEngine.
pub struct DocumentIngester {
    pub state: AppState,
}

#[async_trait::async_trait]
impl thairag_mcp::sync_engine::ContentIngester for DocumentIngester {
    async fn ingest(
        &self,
        workspace_id: WorkspaceId,
        title: &str,
        content: &[u8],
        mime_type: &str,
        existing_doc_id: Option<thairag_core::types::DocId>,
    ) -> thairag_core::error::Result<thairag_core::types::DocId> {
        use thairag_core::models::{DocStatus, Document};
        use thairag_document::converter::MarkdownConverter;

        let doc_id = existing_doc_id.unwrap_or_default();
        let now = chrono::Utc::now();

        // Insert or update document metadata
        if existing_doc_id.is_none() {
            let doc = Document {
                id: doc_id,
                workspace_id,
                title: title.to_string(),
                mime_type: mime_type.to_string(),
                size_bytes: content.len() as i64,
                status: DocStatus::Processing,
                chunk_count: 0,
                error_message: None,
                processing_step: None,
                created_at: now,
                updated_at: now,
            };
            self.state.km_store.insert_document(doc)?;
        } else {
            self.state
                .km_store
                .update_document_status(doc_id, DocStatus::Processing, 0, None)?;
        }

        // Save original bytes + markdown conversion
        let converter = MarkdownConverter::new();
        match converter.convert_with_stats(content, mime_type) {
            Ok(result) => {
                let _ = self.state.km_store.save_document_blob(
                    doc_id,
                    Some(content.to_vec()),
                    Some(result.text),
                    result.image_count,
                    result.table_count,
                );
            }
            Err(_) => {
                let _ = self.state.km_store.save_document_blob(
                    doc_id,
                    Some(content.to_vec()),
                    None,
                    0,
                    0,
                );
            }
        }

        // Process through document pipeline (convert + chunk)
        let p = self.state.providers();
        let chunks = p
            .document_pipeline
            .process(content, mime_type, doc_id, workspace_id, None)
            .await?;

        let chunk_count = chunks.len();

        // Embed + index
        p.search_engine.index_chunks(&chunks).await?;

        // Mark as ready
        let _ = self.state.km_store.update_document_status(
            doc_id,
            DocStatus::Ready,
            chunk_count as i64,
            None,
        );

        tracing::info!(%doc_id, %workspace_id, chunk_count, title, "MCP content ingested");
        Ok(doc_id)
    }
}
