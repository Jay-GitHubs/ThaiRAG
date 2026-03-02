use axum::extract::{Multipart, Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use thairag_core::ThaiRagError;
use thairag_core::types::DocId;
use tracing::info;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::ApiError;

#[derive(Deserialize)]
pub struct IngestRequest {
    pub title: String,
    pub content: String,
    #[serde(default = "default_mime")]
    pub mime_type: String,
}

fn default_mime() -> String {
    "text/plain".to_string()
}

#[derive(Serialize)]
pub struct IngestResponse {
    pub doc_id: Uuid,
    pub chunks: usize,
}

pub async fn ingest_document(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    let workspace_id = thairag_core::types::WorkspaceId(workspace_id);
    let doc_id = DocId::new();

    info!(
        %doc_id,
        %workspace_id,
        title = %body.title,
        mime_type = %body.mime_type,
        "Ingesting document"
    );

    // Convert + chunk
    let chunks = state
        .document_pipeline
        .process(body.content.as_bytes(), &body.mime_type, doc_id, workspace_id)
        .map_err(|e| ApiError(ThaiRagError::Validation(e.to_string())))?;

    let chunk_count = chunks.len();

    // Embed + index into both vector store and Tantivy
    state
        .search_engine
        .index_chunks(&chunks)
        .await
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;

    info!(%doc_id, chunk_count, "Document ingested successfully");

    Ok(Json(IngestResponse {
        doc_id: doc_id.0,
        chunks: chunk_count,
    }))
}

pub async fn upload_document(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<IngestResponse>, ApiError> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut file_content_type: Option<String> = None;
    let mut title: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(ThaiRagError::Validation(format!("Invalid multipart data: {e}"))))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                file_content_type = field.content_type().map(|s| s.to_string());
                file_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            ApiError(ThaiRagError::Validation(format!(
                                "Failed to read file: {e}"
                            )))
                        })?
                        .to_vec(),
                );
            }
            "title" => {
                title = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| {
                            ApiError(ThaiRagError::Validation(format!(
                                "Failed to read title: {e}"
                            )))
                        })?,
                );
            }
            _ => {} // ignore unknown fields
        }
    }

    let bytes = file_bytes
        .ok_or_else(|| ApiError(ThaiRagError::Validation("Missing 'file' field".into())))?;

    let title = title.unwrap_or_else(|| {
        file_name
            .as_deref()
            .unwrap_or("Untitled")
            .to_string()
    });

    // Determine MIME type: explicit content-type (ignoring octet-stream) → extension → text/plain
    let mime_type = file_content_type
        .filter(|ct| ct != "application/octet-stream")
        .or_else(|| {
            file_name
                .as_deref()
                .and_then(|name| name.rsplit('.').next())
                .and_then(mime_from_extension)
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "text/plain".to_string());

    let workspace_id = thairag_core::types::WorkspaceId(workspace_id);
    let doc_id = DocId::new();

    info!(
        %doc_id,
        %workspace_id,
        %title,
        %mime_type,
        size = bytes.len(),
        "Uploading document"
    );

    // Convert + chunk
    let chunks = state
        .document_pipeline
        .process(&bytes, &mime_type, doc_id, workspace_id)
        .map_err(|e| ApiError(ThaiRagError::Validation(e.to_string())))?;

    let chunk_count = chunks.len();

    // Embed + index into both vector store and Tantivy
    state
        .search_engine
        .index_chunks(&chunks)
        .await
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;

    info!(%doc_id, chunk_count, "Document uploaded successfully");

    Ok(Json(IngestResponse {
        doc_id: doc_id.0,
        chunks: chunk_count,
    }))
}

fn mime_from_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "md" | "markdown" => Some("text/markdown"),
        "txt" => Some("text/plain"),
        "html" | "htm" => Some("text/html"),
        "pdf" => Some("application/pdf"),
        "json" => Some("application/json"),
        _ => None,
    }
}
