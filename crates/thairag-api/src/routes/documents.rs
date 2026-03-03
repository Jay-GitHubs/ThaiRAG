use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thairag_auth::AuthClaims;
use thairag_core::models::Document;
use thairag_core::permission::Role;
use thairag_core::types::{DocId, UserId, WorkspaceId};
use thairag_core::ThaiRagError;
use tracing::info;
use uuid::Uuid;

use thairag_document::converter::SUPPORTED_MIME_TYPES;

use crate::app_state::AppState;
use crate::error::ApiError;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub mime_type: String,
    pub size_bytes: i64,
}

// ── Permission helper ───────────────────────────────────────────────

fn resolve_doc_perm(
    claims: &AuthClaims,
    state: &AppState,
    workspace_id: WorkspaceId,
) -> Result<DocPermCheck, ApiError> {
    if claims.sub == "anonymous" {
        return Ok(DocPermCheck::AuthDisabled);
    }
    let org_id = state.km_store.org_id_for_workspace(workspace_id)?;
    let user_id = claims
        .sub
        .parse::<Uuid>()
        .map(UserId)
        .map_err(|_| ApiError(ThaiRagError::Auth("Invalid user ID in token".into())))?;
    match state.km_store.get_user_role_for_org(user_id, org_id) {
        Some(role) => Ok(DocPermCheck::Role(role)),
        None => Ok(DocPermCheck::NoPermission),
    }
}

enum DocPermCheck {
    AuthDisabled,
    Role(Role),
    NoPermission,
}

fn require_doc(
    perm: &DocPermCheck,
    check: fn(&Role) -> bool,
    action: &str,
) -> Result<(), ApiError> {
    match perm {
        DocPermCheck::AuthDisabled => Ok(()),
        DocPermCheck::Role(role) if check(role) => Ok(()),
        DocPermCheck::Role(_) | DocPermCheck::NoPermission => Err(ApiError(
            ThaiRagError::Authorization(format!("Insufficient permission: {action}")),
        )),
    }
}

// ── Handlers ────────────────────────────────────────────────────────

pub async fn ingest_document(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<IngestRequest>,
) -> Result<(StatusCode, Json<IngestResponse>), ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "ingest document")?;

    // Validate MIME type
    validate_mime_type(&body.mime_type)?;

    let doc_id = DocId::new();
    let size_bytes = body.content.len() as i64;

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

    // Store document metadata in KM store
    let now = Utc::now();
    let mime_type = body.mime_type;
    let doc = Document {
        id: doc_id,
        workspace_id,
        title: body.title,
        mime_type: mime_type.clone(),
        size_bytes,
        created_at: now,
        updated_at: now,
    };
    state.km_store.insert_document(doc)?;

    info!(%doc_id, chunk_count, "Document ingested successfully");

    Ok((
        StatusCode::CREATED,
        Json(IngestResponse {
            doc_id: doc_id.0,
            chunks: chunk_count,
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

    // Validate MIME type
    validate_mime_type(&mime_type)?;

    let workspace_id = workspace_id_typed;
    let doc_id = DocId::new();
    let size_bytes = bytes.len() as i64;

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

    // Store document metadata in KM store
    let now = Utc::now();
    let doc = Document {
        id: doc_id,
        workspace_id,
        title: title.clone(),
        mime_type: mime_type.clone(),
        size_bytes,
        created_at: now,
        updated_at: now,
    };
    state.km_store.insert_document(doc)?;

    info!(%doc_id, chunk_count, "Document uploaded successfully");

    Ok((
        StatusCode::CREATED,
        Json(IngestResponse {
            doc_id: doc_id.0,
            chunks: chunk_count,
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
    let _ = state.search_engine.delete_doc(doc_id).await;
    Ok(StatusCode::NO_CONTENT)
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
        _ => None,
    }
}
