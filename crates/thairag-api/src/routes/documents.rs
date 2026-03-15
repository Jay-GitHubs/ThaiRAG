use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thairag_auth::AuthClaims;
use thairag_core::models::{DocStatus, Document};
use thairag_core::permission::Role;
use thairag_core::types::{DocId, WorkspaceId};
use thairag_core::ThaiRagError;
use tracing::{info, warn};
use uuid::Uuid;

use thairag_document::converter::{MarkdownConverter, SUPPORTED_MIME_TYPES};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::km::{ListResponse, PaginationParams, paginate};

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
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub mime_type: String,
    pub size_bytes: i64,
}

// ── Permission helper ───────────────────────────────────────────────

use super::km::resolve_perm_ws;

fn resolve_doc_perm(
    claims: &AuthClaims,
    state: &AppState,
    workspace_id: WorkspaceId,
) -> Result<DocPermCheck, ApiError> {
    let ws = state.km_store.get_workspace(workspace_id).map_err(ApiError)?;
    let dept = state.km_store.get_dept(ws.dept_id).map_err(ApiError)?;
    let perm = resolve_perm_ws(claims, state, dept.org_id, ws.dept_id, workspace_id);
    Ok(perm)
}

type DocPermCheck = super::km::PermCheckPublic;

fn require_doc(
    perm: &DocPermCheck,
    check: fn(&Role) -> bool,
    action: &str,
) -> Result<(), ApiError> {
    match perm {
        DocPermCheck::AuthDisabled | DocPermCheck::SuperAdmin => Ok(()),
        DocPermCheck::Role(role) if check(role) => Ok(()),
        DocPermCheck::Role(_) | DocPermCheck::NoPermission => Err(ApiError(
            ThaiRagError::Authorization(format!("Insufficient permission: {action}")),
        )),
    }
}

// ── Background processing helper ────────────────────────────────────

/// Process document (convert → chunk → embed → index).
/// Small documents (< 1MB) are processed inline for immediate response.
/// Large documents are spawned as background tasks.
async fn process_document(
    state: AppState,
    doc_id: DocId,
    workspace_id: WorkspaceId,
    bytes: Vec<u8>,
    mime_type: String,
    is_large: bool,
) -> usize {
    if is_large {
        // Large file: process in background, return immediately
        tokio::spawn(async move {
            process_document_inner(state, doc_id, workspace_id, bytes, mime_type).await;
        });
        0 // chunk count unknown yet
    } else {
        // Small file: process inline
        let (chunk_count, error) =
            process_document_inner(state, doc_id, workspace_id, bytes, mime_type).await;
        if let Some(err) = error {
            tracing::error!(%doc_id, %err, "Small file processing failed");
        }
        chunk_count
    }
}

async fn process_document_inner(
    state: AppState,
    doc_id: DocId,
    workspace_id: WorkspaceId,
    bytes: Vec<u8>,
    mime_type: String,
) -> (usize, Option<String>) {
    let p = state.providers();

    // Step callback: update the document's processing_step in the store
    let km = state.km_store.clone();
    let step_doc_id = doc_id;
    let on_step: Option<thairag_document::pipeline::StepCallback> =
        Some(std::sync::Arc::new(move |step: &str| {
            let _ = km.update_document_step(step_doc_id, Some(step.to_string()));
        }));

    // Save original file bytes + convert to markdown for preview
    {
        let converter = MarkdownConverter::new();
        match converter.convert_with_stats(&bytes, &mime_type) {
            Ok(result) => {
                let _ = state.km_store.save_document_blob(
                    doc_id,
                    Some(bytes.clone()),
                    Some(result.text),
                    result.image_count,
                    result.table_count,
                );
            }
            Err(_) => {
                // Still save original bytes even if conversion fails
                let _ = state.km_store.save_document_blob(
                    doc_id,
                    Some(bytes.clone()),
                    None,
                    0,
                    0,
                );
            }
        }
    }

    // Convert + chunk (AI or mechanical depending on config)
    let chunks = match p
        .document_pipeline
        .process(&bytes, &mime_type, doc_id, workspace_id, on_step)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Document processing failed: {e}");
            warn!(%doc_id, %msg);
            let _ = state.km_store.update_document_step(doc_id, None);
            let _ = state.km_store.update_document_status(
                doc_id,
                DocStatus::Failed,
                0,
                Some(msg.clone()),
            );
            return (0, Some(msg));
        }
    };

    let chunk_count = chunks.len();

    // Embed + index
    let _ = state.km_store.update_document_step(doc_id, Some("indexing".into()));
    if let Err(e) = p.search_engine.index_chunks(&chunks).await {
        let msg = format!("Indexing failed: {e}");
        warn!(%doc_id, %msg);
        let _ = state.km_store.update_document_step(doc_id, None);
        let _ = state.km_store.update_document_status(
            doc_id,
            DocStatus::Failed,
            0,
            Some(msg.clone()),
        );
        return (0, Some(msg));
    }

    // Mark as ready and clear processing step
    let _ = state.km_store.update_document_step(doc_id, None);
    let _ = state.km_store.update_document_status(
        doc_id,
        DocStatus::Ready,
        chunk_count as i64,
        None,
    );

    info!(%doc_id, chunk_count, "Document processed successfully");
    (chunk_count, None)
}

// ── Size threshold for background processing ────────────────────────

const BACKGROUND_THRESHOLD: usize = 1024 * 1024; // 1MB

// ── Handlers ────────────────────────────────────────────────────────

pub async fn ingest_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    AppJson(body): AppJson<IngestRequest>,
) -> Result<(StatusCode, Json<IngestResponse>), ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "ingest document")?;

    validate_mime_type(&body.mime_type)?;

    // LLM10: Enforce max upload size
    let max_bytes = state.config.document.max_upload_size_mb * 1024 * 1024;
    if body.content.len() > max_bytes {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Document too large: {} bytes (max {} MB)",
            body.content.len(),
            state.config.document.max_upload_size_mb
        ))));
    }

    let doc_id = DocId::new();
    let size_bytes = body.content.len() as i64;
    let is_large = body.content.len() > BACKGROUND_THRESHOLD;

    info!(
        %doc_id, %workspace_id, title = %body.title, mime_type = %body.mime_type,
        size = body.content.len(), background = is_large, "Ingesting document"
    );

    // Insert document metadata first (as processing or ready)
    let now = Utc::now();
    let status = if is_large { DocStatus::Processing } else { DocStatus::Processing };
    let mime_type = body.mime_type.clone();
    let doc = Document {
        id: doc_id,
        workspace_id,
        title: body.title,
        mime_type: mime_type.clone(),
        size_bytes,
        status,
        chunk_count: 0,
        error_message: None,
        processing_step: None,
        created_at: now,
        updated_at: now,
    };
    state.km_store.insert_document(doc)?;

    let chunk_count = process_document(
        state, doc_id, workspace_id,
        body.content.into_bytes(), mime_type.clone(), is_large,
    ).await;

    let resp_status = if is_large { StatusCode::ACCEPTED } else { StatusCode::CREATED };
    Ok((
        resp_status,
        Json(IngestResponse {
            doc_id: doc_id.0,
            chunks: chunk_count,
            status: if is_large { "processing".into() } else { "ready".into() },
            filename: None,
            mime_type,
            size_bytes,
        }),
    ))
}

pub async fn upload_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<IngestResponse>), ApiError> {
    let workspace_id_typed = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id_typed)?;
    require_doc(&perm, Role::can_write, "upload document")?;

    // Stream multipart to temp file for large uploads
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

    // LLM10: Enforce max upload size
    let max_bytes = state.config.document.max_upload_size_mb * 1024 * 1024;
    if bytes.len() > max_bytes {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "File too large: {} bytes (max {} MB)",
            bytes.len(),
            state.config.document.max_upload_size_mb
        ))));
    }

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

    validate_mime_type(&mime_type)?;

    let workspace_id = workspace_id_typed;
    let doc_id = DocId::new();
    let size_bytes = bytes.len() as i64;
    let is_large = bytes.len() > BACKGROUND_THRESHOLD;

    info!(
        %doc_id, %workspace_id, %title, %mime_type,
        size = bytes.len(), background = is_large, "Uploading document"
    );

    // Insert document metadata first
    let now = Utc::now();
    let doc = Document {
        id: doc_id,
        workspace_id,
        title: title.clone(),
        mime_type: mime_type.clone(),
        size_bytes,
        status: DocStatus::Processing,
        chunk_count: 0,
        error_message: None,
        processing_step: None,
        created_at: now,
        updated_at: now,
    };
    state.km_store.insert_document(doc)?;

    let chunk_count = process_document(
        state, doc_id, workspace_id, bytes, mime_type.clone(), is_large,
    ).await;

    let resp_status = if is_large { StatusCode::ACCEPTED } else { StatusCode::CREATED };
    Ok((
        resp_status,
        Json(IngestResponse {
            doc_id: doc_id.0,
            chunks: chunk_count,
            status: if is_large { "processing".into() } else { "ready".into() },
            filename: file_name,
            mime_type,
            size_bytes,
        }),
    ))
}

pub async fn list_documents(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListResponse<Document>>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "list documents")?;
    let docs = state.km_store.list_documents_in_workspace(workspace_id);
    let (data, total) = paginate(docs, &params);
    Ok(Json(ListResponse { data, total }))
}

pub async fn get_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Document>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "read document")?;
    let doc = state.km_store.get_document(DocId(doc_id))?;
    Ok(Json(doc))
}

pub async fn delete_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_delete, "delete document")?;
    let doc_id = DocId(doc_id);
    state.km_store.delete_document(doc_id)?;
    let _ = state.providers().search_engine.delete_doc(doc_id).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── Document content / download / chunks / reprocess ──────────────────

#[derive(Serialize)]
pub struct DocumentContentResponse {
    pub doc_id: Uuid,
    pub converted_text: Option<String>,
    pub image_count: i32,
    pub table_count: i32,
}

pub async fn get_document_content(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DocumentContentResponse>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "read document content")?;

    let doc_id = DocId(doc_id);
    // Verify document exists
    let _ = state.km_store.get_document(doc_id)?;

    let converted_text = state.km_store.get_document_content(doc_id).unwrap_or(None);
    let (image_count, table_count) = state.km_store.get_document_blob_stats(doc_id).unwrap_or((0, 0));

    Ok(Json(DocumentContentResponse {
        doc_id: doc_id.0,
        converted_text,
        image_count,
        table_count,
    }))
}

pub async fn download_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::response::Response, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "download document")?;

    let doc_id_typed = DocId(doc_id);
    let doc = state.km_store.get_document(doc_id_typed)?;
    let file_bytes = state.km_store.get_document_file(doc_id_typed)
        .map_err(ApiError)?
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Original file not stored".into())))?;

    let filename = doc.title.replace('"', "_");
    Ok(axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", doc.mime_type)
        .header("content-disposition", format!("attachment; filename=\"{filename}\""))
        .header("content-length", file_bytes.len().to_string())
        .body(axum::body::Body::from(file_bytes))
        .unwrap())
}

#[derive(Serialize)]
pub struct ChunkInfo {
    pub chunk_id: String,
    pub text: String,
    pub page: Option<i32>,
    pub index: usize,
}

#[derive(Serialize)]
pub struct ChunksResponse {
    pub doc_id: Uuid,
    pub chunks: Vec<ChunkInfo>,
    pub total: usize,
}

pub async fn get_document_chunks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ChunksResponse>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "read document chunks")?;

    let doc_id_typed = DocId(doc_id);
    let doc = state.km_store.get_document(doc_id_typed)?;

    // Get stored converted text and re-chunk it for preview
    let converted = state.km_store.get_document_content(doc_id_typed)
        .unwrap_or(None);

    let chunks: Vec<ChunkInfo> = if let Some(text) = converted {
        let p = state.providers();
        let doc_chunks = p.document_pipeline
            .process(text.as_bytes(), "text/plain", doc_id_typed, workspace_id, None)
            .await
            .unwrap_or_default();

        doc_chunks
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let page = c.metadata.as_ref()
                    .and_then(|m| m.page_numbers.as_ref())
                    .and_then(|pages| pages.first())
                    .map(|&p| p as i32);
                ChunkInfo {
                    chunk_id: c.chunk_id.0.to_string(),
                    text: c.content,
                    page,
                    index: i,
                }
            })
            .collect()
    } else {
        // No converted text; return chunk count from doc metadata
        let count = doc.chunk_count as usize;
        (0..count)
            .map(|i| ChunkInfo {
                chunk_id: format!("{}-{i}", doc_id),
                text: String::new(),
                page: None,
                index: i,
            })
            .collect()
    };

    let total = chunks.len();
    Ok(Json(ChunksResponse {
        doc_id,
        chunks,
        total,
    }))
}

pub async fn reprocess_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "reprocess document")?;

    let doc_id_typed = DocId(doc_id);
    let doc = state.km_store.get_document(doc_id_typed)?;

    // Get original file bytes
    let file_bytes = state.km_store.get_document_file(doc_id_typed)
        .map_err(ApiError)?
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Original file not stored; cannot reprocess".into())))?;

    // Delete old chunks from search index
    let _ = state.providers().search_engine.delete_doc(doc_id_typed).await;

    // Mark as processing
    let _ = state.km_store.update_document_status(
        doc_id_typed, DocStatus::Processing, 0, None,
    );

    info!(%doc_id, "Reprocessing document");

    // Reprocess in background
    let mime = doc.mime_type.clone();
    tokio::spawn(async move {
        process_document_inner(state, doc_id_typed, workspace_id, file_bytes, mime).await;
    });

    Ok(Json(serde_json::json!({
        "doc_id": doc_id,
        "status": "processing",
        "message": "Document reprocessing started"
    })))
}

fn validate_mime_type(mime_type: &str) -> Result<(), ApiError> {
    if !SUPPORTED_MIME_TYPES.contains(&mime_type) {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Unsupported MIME type: {mime_type}. Supported types: {}",
            SUPPORTED_MIME_TYPES.join(", ")
        ))));
    }
    Ok(())
}

fn mime_from_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "md" | "markdown" => Some("text/markdown"),
        "txt" => Some("text/plain"),
        "html" | "htm" => Some("text/html"),
        "pdf" => Some("application/pdf"),
        "json" => Some("application/json"),
        "csv" => Some("text/csv"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        _ => None,
    }
}
