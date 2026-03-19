use std::time::Duration;

use async_trait::async_trait;
use thairag_core::error::{Result, ThaiRagError};
use thairag_core::types::{
    McpConnectorConfig, McpResource, McpResourceContent, McpToolInfo, McpTransport,
};
use tracing::{debug, info};

use rmcp::model::{CallToolRequestParams, RawContent};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};

/// MCP client backed by the `rmcp` crate.
/// Supports both stdio (child process) and streamable HTTP transports.
pub struct RmcpClient {
    config: McpConnectorConfig,
    connect_timeout: Duration,
    read_timeout: Duration,
    session: Option<RunningService<RoleClient, ()>>,
}

impl RmcpClient {
    pub fn new(
        config: McpConnectorConfig,
        connect_timeout: Duration,
        read_timeout: Duration,
    ) -> Self {
        Self {
            config,
            connect_timeout,
            read_timeout,
            session: None,
        }
    }

    fn session(&self) -> Result<&RunningService<RoleClient, ()>> {
        self.session
            .as_ref()
            .ok_or_else(|| ThaiRagError::Internal("MCP client not connected".into()))
    }
}

#[async_trait]
impl thairag_core::traits::McpClient for RmcpClient {
    async fn connect(&mut self) -> Result<()> {
        info!(
            connector = %self.config.name,
            transport = ?self.config.transport,
            "Connecting to MCP server"
        );

        let service: RunningService<RoleClient, ()> = match &self.config.transport {
            McpTransport::Stdio => {
                let command = self.config.command.as_deref().ok_or_else(|| {
                    ThaiRagError::Internal("stdio transport requires a command".into())
                })?;

                let mut cmd = tokio::process::Command::new(command);
                cmd.args(&self.config.args);
                for (k, v) in &self.config.env {
                    cmd.env(k, v);
                }

                let transport = TokioChildProcess::new(cmd).map_err(|e| {
                    ThaiRagError::Internal(format!("Failed to spawn MCP process: {e}"))
                })?;

                tokio::time::timeout(self.connect_timeout, ().serve(transport))
                    .await
                    .map_err(|_| ThaiRagError::Internal("MCP connect timeout".into()))?
                    .map_err(|e| ThaiRagError::Internal(format!("MCP handshake failed: {e}")))?
            }
            McpTransport::Sse => {
                let url =
                    self.config.url.as_deref().ok_or_else(|| {
                        ThaiRagError::Internal("SSE transport requires a url".into())
                    })?;

                let http_config =
                    rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url);
                let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
                    reqwest::Client::new(),
                    http_config,
                );

                tokio::time::timeout(self.connect_timeout, ().serve(transport))
                    .await
                    .map_err(|_| ThaiRagError::Internal("MCP connect timeout".into()))?
                    .map_err(|e| ThaiRagError::Internal(format!("MCP handshake failed: {e}")))?
            }
        };

        debug!(connector = %self.config.name, "MCP handshake complete");
        self.session = Some(service);
        Ok(())
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>> {
        let session = self.session()?;

        let resources = tokio::time::timeout(self.read_timeout, session.list_all_resources())
            .await
            .map_err(|_| ThaiRagError::Internal("list_resources timeout".into()))?
            .map_err(|e| ThaiRagError::Internal(format!("list_resources failed: {e}")))?;

        Ok(resources
            .into_iter()
            .map(|r| McpResource {
                uri: r.raw.uri,
                name: r.raw.name,
                mime_type: r.raw.mime_type,
                description: r.raw.description,
            })
            .collect())
    }

    async fn read_resource(&self, uri: &str) -> Result<McpResourceContent> {
        let session = self.session()?;

        let resp = tokio::time::timeout(
            self.read_timeout,
            session.read_resource(rmcp::model::ReadResourceRequestParams::new(uri)),
        )
        .await
        .map_err(|_| ThaiRagError::Internal("read_resource timeout".into()))?
        .map_err(|e| ThaiRagError::Internal(format!("read_resource failed: {e}")))?;

        let mut data = Vec::new();
        let mut mime_type = "text/plain".to_string();

        for content in &resp.contents {
            match content {
                rmcp::model::ResourceContents::TextResourceContents {
                    text,
                    mime_type: mt,
                    ..
                } => {
                    data.extend_from_slice(text.as_bytes());
                    if let Some(m) = mt {
                        mime_type = m.clone();
                    }
                }
                rmcp::model::ResourceContents::BlobResourceContents {
                    blob,
                    mime_type: mt,
                    ..
                } => {
                    use base64::Engine;
                    if let Ok(decoded) =
                        base64::engine::general_purpose::STANDARD.decode(blob.as_bytes())
                    {
                        data.extend_from_slice(&decoded);
                    }
                    if let Some(m) = mt {
                        mime_type = m.clone();
                    }
                }
            }
        }

        Ok(McpResourceContent {
            uri: uri.to_string(),
            mime_type,
            data,
        })
    }

    async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        let session = self.session()?;

        let tools = tokio::time::timeout(self.read_timeout, session.list_all_tools())
            .await
            .map_err(|_| ThaiRagError::Internal("list_tools timeout".into()))?
            .map_err(|e| ThaiRagError::Internal(format!("list_tools failed: {e}")))?;

        Ok(tools
            .into_iter()
            .map(|t| McpToolInfo {
                name: t.name.into_owned(),
                description: t
                    .description
                    .map(|d: std::borrow::Cow<'_, str>| d.into_owned()),
                input_schema: serde_json::to_value(&*t.input_schema).ok(),
            })
            .collect())
    }

    async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        let session = self.session()?;

        let arguments = if let serde_json::Value::Object(map) = args {
            Some(map)
        } else {
            None
        };

        let resp = tokio::time::timeout(
            self.read_timeout,
            session.call_tool({
                let mut params = CallToolRequestParams::new(name.to_string());
                if let Some(args) = arguments {
                    params = params.with_arguments(args);
                }
                params
            }),
        )
        .await
        .map_err(|_| ThaiRagError::Internal("call_tool timeout".into()))?
        .map_err(|e| ThaiRagError::Internal(format!("call_tool failed: {e}")))?;

        let results: Vec<serde_json::Value> = resp
            .content
            .into_iter()
            .filter_map(|c| match &c.raw {
                RawContent::Text(t) => serde_json::from_str(&t.text)
                    .ok()
                    .or_else(|| Some(serde_json::Value::String(t.text.clone()))),
                _ => None,
            })
            .collect();

        Ok(serde_json::Value::Array(results))
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut session) = self.session.take() {
            let _ = session.close().await;
        }
        info!(connector = %self.config.name, "Disconnected from MCP server");
        Ok(())
    }
}
