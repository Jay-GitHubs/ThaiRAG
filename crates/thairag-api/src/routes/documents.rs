use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::models::{DocStatus, Document};
use thairag_core::permission::Role;
use thairag_core::types::{
    DocId, Job, JobId, JobKind, JobStatus, UserId, WebhookEvent, WorkspaceId,
};
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
    /// Optional source URL for scheduled re-ingestion.
    #[serde(default)]
    pub source_url: Option<String>,
    /// Optional refresh interval (e.g., "1h", "6h", "1d", "7d").
    #[serde(default)]
    pub refresh_schedule: Option<String>,
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

use super::km::{resolve_perm_ws, user_id_from_claims_pub};
use thairag_core::types::AclPermission;

fn resolve_doc_perm(
    claims: &AuthClaims,
    state: &AppState,
    workspace_id: WorkspaceId,
) -> Result<DocPermCheck, ApiError> {
    let ws = state
        .km_store
        .get_workspace(workspace_id)
        .map_err(ApiError)?;
    let dept = state.km_store.get_dept(ws.dept_id).map_err(ApiError)?;
    let perm = resolve_perm_ws(claims, state, dept.org_id, ws.dept_id, workspace_id);
    // If hierarchical permission says NoPermission, fall back to workspace ACLs
    if matches!(perm, DocPermCheck::NoPermission)
        && let Some(user_id) = user_id_from_claims_pub(claims)
        && let Some(acl_perm) = state.km_store.get_user_workspace_acl(user_id, workspace_id)
    {
        let role = match acl_perm {
            AclPermission::Read => Role::Viewer,
            AclPermission::Write => Role::Editor,
            AclPermission::Admin => Role::Admin,
        };
        return Ok(DocPermCheck::Role(role));
    }
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
/// Fast paths (small, text-only) are processed inline for immediate response.
/// Slow paths (large, PDF/image vision, or AI preprocessing) are submitted to
/// the job queue so the upload response returns immediately. See
/// [`should_process_in_background`].
async fn process_document(
    state: AppState,
    doc_id: DocId,
    workspace_id: WorkspaceId,
    bytes: Vec<u8>,
    mime_type: String,
    background: bool,
) -> usize {
    if background {
        // Large file: submit to job queue
        let job = Job {
            id: JobId(Uuid::new_v4()),
            kind: JobKind::DocumentIngestion,
            status: JobStatus::Queued,
            workspace_id,
            doc_id: Some(doc_id),
            description: format!("Ingest document {doc_id}"),
            created_at: now_ts(),
            started_at: None,
            completed_at: None,
            error: None,
            items_processed: 0,
            items_total: None,
        };
        let job_id = state.job_queue.enqueue(job).await;
        let jq = state.job_queue.clone();
        tokio::spawn(async move {
            jq.mark_running(&job_id).await;
            let (chunks, error) =
                process_document_inner(state.clone(), doc_id, workspace_id, bytes, mime_type).await;
            if let Some(ref err) = error {
                jq.mark_failed(&job_id, err.clone()).await;
                state.webhook_dispatcher.dispatch(
                    WebhookEvent::JobFailed,
                    serde_json::json!({
                        "job_id": job_id.0,
                        "doc_id": doc_id.0,
                        "workspace_id": workspace_id.0,
                        "error": err,
                    }),
                );
            } else {
                jq.mark_completed(&job_id, chunks).await;
                state.webhook_dispatcher.dispatch(
                    WebhookEvent::JobCompleted,
                    serde_json::json!({
                        "job_id": job_id.0,
                        "doc_id": doc_id.0,
                        "workspace_id": workspace_id.0,
                        "chunks_indexed": chunks,
                    }),
                );
                state.webhook_dispatcher.dispatch(
                    WebhookEvent::DocumentIngested,
                    serde_json::json!({
                        "doc_id": doc_id.0,
                        "workspace_id": workspace_id.0,
                        "chunks_indexed": chunks,
                    }),
                );
            }
        });
        0 // chunk count unknown yet
    } else {
        // Small file: process inline
        let (chunk_count, error) =
            process_document_inner(state.clone(), doc_id, workspace_id, bytes, mime_type).await;
        if let Some(ref err) = error {
            tracing::error!(%doc_id, %err, "Small file processing failed");
        } else {
            state.webhook_dispatcher.dispatch(
                WebhookEvent::DocumentIngested,
                serde_json::json!({
                    "doc_id": doc_id.0,
                    "workspace_id": workspace_id.0,
                    "chunks_indexed": chunk_count,
                }),
            );
        }
        chunk_count
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Panic-guarded entry point. Spawns the real work as a child task so a
/// panic in the converter/chunker/plugins (e.g. an `unwrap` on a malformed
/// PDF, a third-party crate's internal assertion) is caught instead of
/// leaving the document stuck in `Processing` forever.
///
/// On panic or unexpected task error, the document is marked `Failed`
/// with a structured `panic_during_ingest:` / `ingest_task_error:` message
/// so admin UIs can surface a clear reason.
async fn process_document_inner(
    state: AppState,
    doc_id: DocId,
    workspace_id: WorkspaceId,
    bytes: Vec<u8>,
    mime_type: String,
) -> (usize, Option<String>) {
    let state_for_panic = state.clone();
    let handle = tokio::spawn(process_document_inner_impl(
        state,
        doc_id,
        workspace_id,
        bytes,
        mime_type,
    ));

    match handle.await {
        Ok(pair) => pair,
        Err(join_err) if join_err.is_panic() => {
            let panic_msg = panic_payload_to_string(join_err.into_panic());
            let err = format!("panic_during_ingest: {panic_msg}");
            tracing::error!(%doc_id, panic = %panic_msg, "PANIC during document processing");
            mark_doc_failed_best_effort(&state_for_panic, doc_id, &err).await;
            (0, Some(err))
        }
        Err(join_err) => {
            // Task was cancelled or otherwise did not complete normally.
            // Most likely runtime shutdown — the worst outcome is a doc
            // briefly stuck Processing until startup reconciliation runs.
            let err = format!("ingest_task_error: {join_err}");
            tracing::error!(%doc_id, error = %join_err, "Ingest task did not complete");
            mark_doc_failed_best_effort(&state_for_panic, doc_id, &err).await;
            (0, Some(err))
        }
    }
}

/// Format a panic payload (from `JoinError::into_panic`) into a string
/// safe to store in the document's `error_message`. Panics typically
/// carry a `String` or `&'static str`; anything else falls back to a
/// generic marker so we never lose the "this was a panic" signal.
fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        "unknown panic payload".to_string()
    }
}

/// Best-effort terminal status write for the panic / task-error paths.
/// Each `let _` is intentional — we are already in an error path and
/// cannot meaningfully recover from a store failure here.
async fn mark_doc_failed_best_effort(state: &AppState, doc_id: DocId, error_message: &str) {
    let _ = state.km_store.update_document_step(doc_id, None, None);
    let _ = state.km_store.update_document_status(
        doc_id,
        DocStatus::Failed,
        0,
        Some(error_message.to_string()),
    );
}

async fn process_document_inner_impl(
    state: AppState,
    doc_id: DocId,
    workspace_id: WorkspaceId,
    bytes: Vec<u8>,
    mime_type: String,
) -> (usize, Option<String>) {
    let p = state.providers();

    // Resolve the document pipeline for this workspace's scope (walking
    // workspace → dept → org → global). An org/dept/workspace that overrides
    // chunking, PDF/OCR, or AI-preprocessing config gets a pipeline built from
    // its effective config; scopes without overrides share the global pipeline.
    // The scoped pipeline already bakes in chunk sizing, so no per-call
    // `ChunkOverrides` are needed (default = transparent no-op).
    let scope = state.resolve_scope_for_workspace(workspace_id);
    let pipeline = state.get_scoped_document_pipeline(&scope);

    // Step callback: record the processing step AND the model that runs it (from
    // the scoped pipeline) so the admin UI can see, live, which model is acting
    // on each stage — and abort the run if it's the wrong one.
    let km = state.km_store.clone();
    let step_doc_id = doc_id;
    let step_pipeline = pipeline.clone();
    let on_step: Option<thairag_document::pipeline::StepCallback> =
        Some(std::sync::Arc::new(move |step: &str| {
            let model = step_pipeline.model_for_step(step);
            let _ = km.update_document_step(step_doc_id, Some(step.to_string()), model);
        }));

    // Apply document plugins (e.g., metadata stripping) before processing
    let bytes =
        crate::plugin_hooks::apply_document_plugins(&state.plugin_registry, &bytes, &mime_type);

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
                let _ = state
                    .km_store
                    .save_document_blob(doc_id, Some(bytes.clone()), None, 0, 0);
            }
        }
    }

    // Convert + chunk (AI or mechanical depending on config). The pipeline
    // returns `EmptyExtraction` when no meaningful content was produced —
    // surface its structured reason/hint instead of swallowing it into a
    // generic failure message so the admin UI can act on it.
    let processed = match pipeline
        .process_to_document(
            &bytes,
            &mime_type,
            doc_id,
            workspace_id,
            on_step,
            thairag_document::pipeline::ChunkOverrides::default(),
        )
        .await
    {
        Ok(d) => d,
        Err(thairag_core::ThaiRagError::EmptyExtraction { reason, hint }) => {
            let msg = format!("empty_extraction[{reason}]: {hint}");
            warn!(%doc_id, reason, hint, "Document produced no chunks");
            let _ = state.km_store.update_document_step(doc_id, None, None);
            let _ = state.km_store.update_document_status(
                doc_id,
                DocStatus::Failed,
                0,
                Some(msg.clone()),
            );
            return (0, Some(msg));
        }
        Err(e) => {
            let msg = format!("Document processing failed: {e}");
            warn!(%doc_id, %msg);
            let _ = state.km_store.update_document_step(doc_id, None, None);
            let _ = state.km_store.update_document_status(
                doc_id,
                DocStatus::Failed,
                0,
                Some(msg.clone()),
            );
            return (0, Some(msg));
        }
    };

    // Smart-PDF path: persist the extracted image blobs and replace the
    // mechanical markdown preview saved above with the canonical semantic
    // markdown. Image ids are already embedded in chunks + markdown.
    if let Some(markdown) = processed.markdown.as_ref() {
        // Clear any prior blobs (covers reprocess) before re-saving.
        let _ = state.km_store.delete_image_blobs_for_doc(doc_id);
        let mut saved_images = 0i32;
        for blob in &processed.images {
            let record = crate::store::ImageBlobRecord {
                image_id: blob.image_id,
                doc_id,
                workspace_id,
                bytes: blob.bytes.clone(),
                mime: blob.mime.clone(),
                width: blob.width,
                height: blob.height,
                page_num: blob.page_num,
                source: crate::store::ImageSource::from_str_lossy(blob.source),
            };
            match state.km_store.save_image_blob(record) {
                Ok(()) => saved_images += 1,
                Err(e) => warn!(%doc_id, error = %e, "Failed to save image blob (non-fatal)"),
            }
        }
        let table_count = processed
            .chunks
            .iter()
            .filter(|c| {
                c.metadata.as_ref().and_then(|m| m.content_type.as_ref())
                    == Some(&thairag_core::types::DocumentContentType::Table)
            })
            .count() as i32;
        let _ = state.km_store.save_document_blob(
            doc_id,
            Some(bytes.clone()),
            Some(markdown.clone()),
            saved_images,
            table_count,
        );
    }

    // Persist processing provenance (path, agents, models, fallback) so the
    // admin UI can show a per-document transparency summary.
    if let Some(provenance) = processed.provenance.clone() {
        let _ = state
            .km_store
            .update_document_provenance(doc_id, Some(provenance));
    }

    let chunks = processed.chunks;

    // Apply chunk plugins (e.g., summary headers) after splitting
    let mut chunks = chunks;
    crate::plugin_hooks::apply_chunk_plugins(&state.plugin_registry, &mut chunks);

    let chunk_count = chunks.len();

    // Defence-in-depth: the pipeline should have caught this, but guard
    // against any future code path that returns Ok(empty). Never mark a
    // document Ready with zero chunks — it would silently become an
    // unsearchable orphan.
    if chunk_count == 0 {
        let msg = "empty_extraction[no_chunks_after_plugins]: pipeline returned no chunks. \
                   Check chunk plugins and document content."
            .to_string();
        warn!(%doc_id, %msg);
        let _ = state.km_store.update_document_step(doc_id, None, None);
        let _ =
            state
                .km_store
                .update_document_status(doc_id, DocStatus::Failed, 0, Some(msg.clone()));
        return (0, Some(msg));
    }

    // Save chunks to DB for Tantivy rebuild on restart
    if let Err(e) = state.km_store.save_chunks(&chunks) {
        warn!(%doc_id, error = %e, "Failed to save chunks to DB (non-fatal)");
    }

    // Compute CLIP image vectors for image-bearing chunks so the search engine
    // can populate the visual-similarity collection. Only runs when visual
    // search is enabled; vectors ride along in `metadata.image_embedding`
    // (transient, never persisted) and are consumed by `index_chunks`.
    if let Some(img_model) = p.image_embedding.as_ref() {
        let blob_bytes: std::collections::HashMap<_, _> = processed
            .images
            .iter()
            .map(|b| (b.image_id, b.bytes.as_slice()))
            .collect();
        let mut targets: Vec<usize> = Vec::new();
        let mut batch: Vec<Vec<u8>> = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            if let Some(id) = chunk.metadata.as_ref().and_then(|m| m.image_blob_id)
                && let Some(bytes) = blob_bytes.get(&id)
            {
                targets.push(i);
                batch.push(bytes.to_vec());
            }
        }
        if !batch.is_empty() {
            match img_model.embed_images(&batch).await {
                Ok(vecs) => {
                    for (idx, vec) in targets.into_iter().zip(vecs) {
                        if let Some(meta) = chunks[idx].metadata.as_mut() {
                            meta.image_embedding = Some(vec);
                        }
                    }
                }
                Err(e) => {
                    warn!(%doc_id, error = %e, "CLIP image embedding failed (non-fatal)")
                }
            }
        }
    }

    // Embed + index
    let _ = state
        .km_store
        .update_document_step(doc_id, Some("indexing".into()), None);
    if let Err(e) = p.search_engine.index_chunks(&chunks).await {
        let msg = format!("Indexing failed: {e}");
        warn!(%doc_id, %msg);
        let _ = state.km_store.update_document_step(doc_id, None, None);
        let _ =
            state
                .km_store
                .update_document_status(doc_id, DocStatus::Failed, 0, Some(msg.clone()));
        return (0, Some(msg));
    }

    // Compute content hash from the converted text
    let content_hash = state
        .km_store
        .get_document_content(doc_id)
        .unwrap_or(None)
        .map(|text| compute_content_hash(text.as_bytes()));

    // Mark as ready, clear processing step, and bump version + content_hash
    let _ = state.km_store.update_document_step(doc_id, None, None);
    let _ =
        state
            .km_store
            .update_document_status(doc_id, DocStatus::Ready, chunk_count as i64, None);

    // Update content_hash and increment version on the document
    if let Ok(doc) = state.km_store.get_document(doc_id) {
        // We update via a status call that's already done; content_hash is stored separately
        // For now, we'll rely on the document's existing version tracking
        let _ = state
            .km_store
            .update_document_version_info(doc_id, doc.version + 1, content_hash);
    }

    // Knowledge graph extraction (background task — never blocks or fails
    // document indexing; the document is already Ready at this point).
    if state.config.knowledge_graph.enabled && state.config.knowledge_graph.extract_on_ingest {
        let km_store = state.km_store.clone();
        let llm_cfg = p.providers_config.llm.clone();
        let max_chunks = state.config.knowledge_graph.max_chunks_per_doc;
        let chunk_texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        info!(%doc_id, "Spawning knowledge graph extraction on ingest");
        tokio::spawn(async move {
            let llm: std::sync::Arc<dyn thairag_core::traits::LlmProvider> =
                std::sync::Arc::from(thairag_provider_llm::create_llm_provider(&llm_cfg));
            let (entities, relations) = crate::knowledge_graph::extract_and_persist_graph(
                &km_store,
                &llm,
                workspace_id,
                doc_id,
                &chunk_texts,
                max_chunks,
            )
            .await;
            info!(
                %doc_id,
                entities,
                relations,
                "Knowledge graph extraction on ingest complete"
            );
        });
    }

    info!(%doc_id, chunk_count, "Document processed successfully");
    (chunk_count, None)
}

// ── Size threshold for background processing ────────────────────────

const BACKGROUND_THRESHOLD: usize = 1024 * 1024; // 1MB

/// Decide whether a document should be processed in the background.
///
/// Foreground processing blocks the upload HTTP response until the whole
/// convert→chunk→embed→index pipeline finishes. That is fine for small,
/// text-only documents, but smart-PDF/image vision and the AI-agent
/// preprocessing pipeline can take a minute or more — long enough for the
/// client connection to drop and the upload modal to appear frozen. Route
/// any of those slow paths to the job queue so the POST returns immediately
/// as `processing` and the table polls for completion.
fn should_process_in_background(
    state: &AppState,
    workspace_id: WorkspaceId,
    mime_type: &str,
    size: usize,
) -> bool {
    if size > BACKGROUND_THRESHOLD {
        return true;
    }
    // PDFs (smart-PDF vision) and images (direct-image vision) are slow
    // regardless of byte size.
    if mime_type == "application/pdf" || mime_type.starts_with("image/") {
        return true;
    }
    // AI-agent preprocessing adds analyzer/converter/quality/enricher LLM
    // round-trips; effective value layers scoped overrides (workspace → dept →
    // org → global) over the file default, so an org that enables AI
    // preprocessing routes its uploads to the background queue.
    let scope = state.resolve_scope_for_workspace(workspace_id);
    crate::store::resolve_setting(state.km_store.as_ref(), "ai_preprocessing.enabled", &scope)
        .and_then(|v| v.parse().ok())
        .unwrap_or(state.config.document.ai_preprocessing.enabled)
}

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

    // Validate refresh_schedule if provided
    if let Some(ref schedule) = body.refresh_schedule
        && !crate::store::is_valid_refresh_schedule(schedule)
    {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid refresh_schedule: '{schedule}'. Use formats like '1h', '6h', '1d', '7d', '30d'"
        ))));
    }

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
    let background =
        should_process_in_background(&state, workspace_id, &body.mime_type, body.content.len());

    info!(
        %doc_id, %workspace_id, title = %body.title, mime_type = %body.mime_type,
        size = body.content.len(), background, "Ingesting document"
    );

    // Insert document metadata first (as processing or ready)
    let now = Utc::now();
    let status = DocStatus::Processing;
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
        processing_provenance: None,
        processing_timeline: None,
        version: 1,
        content_hash: None,
        source_url: body.source_url,
        refresh_schedule: body.refresh_schedule,
        last_refreshed_at: None,
        created_at: now,
        updated_at: now,
    };
    state.km_store.insert_document(doc)?;

    let chunk_count = process_document(
        state,
        doc_id,
        workspace_id,
        body.content.into_bytes(),
        mime_type.clone(),
        background,
    )
    .await;

    let resp_status = if background {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CREATED
    };
    Ok((
        resp_status,
        Json(IngestResponse {
            doc_id: doc_id.0,
            chunks: chunk_count,
            status: if background {
                "processing".into()
            } else {
                "ready".into()
            },
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

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError(ThaiRagError::Validation(format!(
            "Invalid multipart data: {e}"
        )))
    })? {
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
                title = Some(field.text().await.map_err(|e| {
                    ApiError(ThaiRagError::Validation(format!(
                        "Failed to read title: {e}"
                    )))
                })?);
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

    let title = title.unwrap_or_else(|| file_name.as_deref().unwrap_or("Untitled").to_string());

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
    let background = should_process_in_background(&state, workspace_id, &mime_type, bytes.len());

    info!(
        %doc_id, %workspace_id, %title, %mime_type,
        size = bytes.len(), background, "Uploading document"
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
        processing_provenance: None,
        processing_timeline: None,
        version: 1,
        content_hash: None,
        source_url: None,
        refresh_schedule: None,
        last_refreshed_at: None,
        created_at: now,
        updated_at: now,
    };
    state.km_store.insert_document(doc)?;

    let chunk_count = process_document(
        state,
        doc_id,
        workspace_id,
        bytes,
        mime_type.clone(),
        background,
    )
    .await;

    let resp_status = if background {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CREATED
    };
    Ok((
        resp_status,
        Json(IngestResponse {
            doc_id: doc_id.0,
            chunks: chunk_count,
            status: if background {
                "processing".into()
            } else {
                "ready".into()
            },
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
    let _ = state.km_store.delete_chunks_by_doc(doc_id);
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
    let (image_count, table_count) = state
        .km_store
        .get_document_blob_stats(doc_id)
        .unwrap_or((0, 0));

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
    let file_bytes = state
        .km_store
        .get_document_file(doc_id_typed)
        .map_err(ApiError)?
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Original file not stored".into())))?;

    let filename = doc.title.replace('"', "_");
    Ok(axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", doc.mime_type)
        .header(
            "content-disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .header("content-length", file_bytes.len().to_string())
        .body(axum::body::Body::from(file_bytes))
        .unwrap())
}

/// Serve a stored image blob for inline display in the admin UI or for
/// the chat pipeline's multimodal context. The image must belong to the
/// requested document, which must belong to the requested workspace —
/// the same ACL check the chunks endpoint uses.
pub async fn get_document_image(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id, image_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<axum::response::Response, ApiError> {
    let workspace_id_typed = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id_typed)?;
    require_doc(&perm, Role::can_read, "view document image")?;

    let doc_id_typed = DocId(doc_id);
    // Confirm the doc actually lives in this workspace before serving any bytes.
    let doc = state.km_store.get_document(doc_id_typed)?;
    if doc.workspace_id != workspace_id_typed {
        return Err(ApiError(ThaiRagError::NotFound("image not found".into())));
    }

    let image_id_typed = thairag_core::types::ImageId(image_id);
    let record = state
        .km_store
        .get_image_blob(image_id_typed)
        .map_err(ApiError)?
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("image not found".into())))?;

    if record.doc_id != doc_id_typed || record.workspace_id != workspace_id_typed {
        // Belt-and-suspenders: image_id is a UUID and effectively unguessable,
        // but never serve an image cross-document or cross-workspace.
        return Err(ApiError(ThaiRagError::NotFound("image not found".into())));
    }

    Ok(axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", record.mime)
        .header("content-length", record.bytes.len().to_string())
        .header("cache-control", "private, max-age=3600")
        .body(axum::body::Body::from(record.bytes))
        .unwrap())
}

#[derive(Serialize)]
pub struct DocumentImageInfo {
    pub image_id: Uuid,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_num: Option<u32>,
    pub source: String,
}

/// List image-blob metadata for a document. Used by the admin UI to
/// render an image grid on the document detail page. Returns metadata
/// only (no bytes) — clients fetch each image via `get_document_image`.
pub async fn list_document_images(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<DocumentImageInfo>>, ApiError> {
    let workspace_id_typed = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id_typed)?;
    require_doc(&perm, Role::can_read, "list document images")?;

    let doc_id_typed = DocId(doc_id);
    let doc = state.km_store.get_document(doc_id_typed)?;
    if doc.workspace_id != workspace_id_typed {
        return Err(ApiError(ThaiRagError::NotFound(
            "document not found".into(),
        )));
    }

    let metas = state
        .km_store
        .list_image_blobs_for_doc(doc_id_typed)
        .map_err(ApiError)?;

    Ok(Json(
        metas
            .into_iter()
            .map(|m| DocumentImageInfo {
                image_id: m.image_id.0,
                mime: m.mime,
                width: m.width,
                height: m.height,
                page_num: m.page_num,
                source: m.source.as_str().to_string(),
            })
            .collect(),
    ))
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

    // Return the *persisted* chunks as actually indexed. We must NOT re-run the
    // document pipeline here: with AI preprocessing enabled, `process()` invokes
    // the LLM agents synchronously inside this request, which makes the preview
    // hang for seconds-to-minutes (or forever while a model loads). Reading the
    // stored chunks is both fast and faithful to what search actually sees.
    let stored = state.km_store.load_chunks_by_doc(doc_id_typed);

    let chunks: Vec<ChunkInfo> = if !stored.is_empty() {
        stored
            .into_iter()
            .enumerate()
            .map(|(i, c)| ChunkInfo {
                chunk_id: c.chunk_id.0.to_string(),
                text: c.content,
                page: None,
                index: i,
            })
            .collect()
    } else {
        // No persisted chunks (older docs predating chunk storage); fall back to
        // the count from doc metadata so the modal still reports something.
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

    // Save current version before reprocessing
    let user_id: Option<UserId> = claims.sub.parse().ok().map(UserId);
    save_current_version(&state, doc_id_typed, user_id);

    // Get original file bytes
    let file_bytes = state
        .km_store
        .get_document_file(doc_id_typed)
        .map_err(ApiError)?
        .ok_or_else(|| {
            ApiError(ThaiRagError::NotFound(
                "Original file not stored; cannot reprocess".into(),
            ))
        })?;

    // Delete old chunks from search index AND from the SQL store. Without
    // the SQL delete, the document_chunks table accumulates dead rows that
    // load_all_chunks feeds back into Tantivy on the next restart, doubling
    // or tripling BM25 hits for every reprocessed doc.
    let _ = state
        .providers()
        .search_engine
        .delete_doc(doc_id_typed)
        .await;
    let _ = state.km_store.delete_chunks_by_doc(doc_id_typed);

    // Mark as processing
    let _ = state
        .km_store
        .update_document_status(doc_id_typed, DocStatus::Processing, 0, None);

    info!(%doc_id, "Reprocessing document");

    // Reprocess via job queue
    let mime = doc.mime_type.clone();
    let job = Job {
        id: JobId(Uuid::new_v4()),
        kind: JobKind::DocumentReprocess,
        status: JobStatus::Queued,
        workspace_id,
        doc_id: Some(doc_id_typed),
        description: format!("Reprocess document {doc_id}"),
        created_at: now_ts(),
        started_at: None,
        completed_at: None,
        error: None,
        items_processed: 0,
        items_total: None,
    };
    let job_id = state.job_queue.enqueue(job).await;
    let jq = state.job_queue.clone();
    tokio::spawn(async move {
        jq.mark_running(&job_id).await;
        let (chunks, error) =
            process_document_inner(state.clone(), doc_id_typed, workspace_id, file_bytes, mime)
                .await;
        if let Some(ref err) = error {
            jq.mark_failed(&job_id, err.clone()).await;
            state.webhook_dispatcher.dispatch(
                WebhookEvent::JobFailed,
                serde_json::json!({
                    "job_id": job_id.0,
                    "doc_id": doc_id_typed.0,
                    "workspace_id": workspace_id.0,
                    "error": err,
                }),
            );
        } else {
            jq.mark_completed(&job_id, chunks).await;
            state.webhook_dispatcher.dispatch(
                WebhookEvent::JobCompleted,
                serde_json::json!({
                    "job_id": job_id.0,
                    "doc_id": doc_id_typed.0,
                    "workspace_id": workspace_id.0,
                    "chunks_indexed": chunks,
                }),
            );
        }
    });

    Ok(Json(serde_json::json!({
        "doc_id": doc_id,
        "job_id": job_id.0,
        "status": "processing",
        "message": "Document reprocessing started"
    })))
}

/// Reprocess all ready documents in a workspace (e.g., after embedding model change).
/// Clears all vectors first, then re-embeds each document in the background.
pub async fn reprocess_all_documents(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "reprocess documents")?;

    // Get all docs in this workspace
    let all_docs = state.km_store.list_documents_in_workspace(workspace_id);
    let ready_docs: Vec<_> = all_docs
        .into_iter()
        .filter(|d| d.status == DocStatus::Ready || d.status == DocStatus::Failed)
        .collect();

    if ready_docs.is_empty() {
        return Ok(Json(serde_json::json!({
            "queued": 0,
            "message": "No documents to reprocess"
        })));
    }

    let count = ready_docs.len();

    // Create a batch job for tracking
    let batch_job = Job {
        id: JobId(Uuid::new_v4()),
        kind: JobKind::BatchReprocess,
        status: JobStatus::Running,
        workspace_id,
        doc_id: None,
        description: format!("Batch reprocess {count} documents"),
        created_at: now_ts(),
        started_at: Some(now_ts()),
        completed_at: None,
        error: None,
        items_processed: 0,
        items_total: Some(count),
    };
    let batch_job_id = state.job_queue.enqueue(batch_job).await;

    // Process each document in background via job queue
    let mut queued = 0;
    for doc in ready_docs {
        let doc_id = doc.id;
        let mime = doc.mime_type.clone();
        let file_bytes = match state.km_store.get_document_file(doc_id) {
            Ok(Some(bytes)) => bytes,
            _ => {
                warn!(%doc_id, "Skipping reprocess: no original file stored");
                continue;
            }
        };

        // Delete old chunks from search index AND SQL store. See
        // single-doc reprocess for why both deletes are required.
        let _ = state.providers().search_engine.delete_doc(doc_id).await;
        let _ = state.km_store.delete_chunks_by_doc(doc_id);

        // Mark as processing
        let _ = state
            .km_store
            .update_document_status(doc_id, DocStatus::Processing, 0, None);

        let job = Job {
            id: JobId(Uuid::new_v4()),
            kind: JobKind::DocumentReprocess,
            status: JobStatus::Queued,
            workspace_id,
            doc_id: Some(doc_id),
            description: format!("Reprocess document {doc_id}"),
            created_at: now_ts(),
            started_at: None,
            completed_at: None,
            error: None,
            items_processed: 0,
            items_total: None,
        };
        let job_id = state.job_queue.enqueue(job).await;
        let jq = state.job_queue.clone();
        let s = state.clone();
        tokio::spawn(async move {
            jq.mark_running(&job_id).await;
            let (chunks, error) =
                process_document_inner(s.clone(), doc_id, workspace_id, file_bytes, mime).await;
            if let Some(ref err) = error {
                jq.mark_failed(&job_id, err.clone()).await;
                s.webhook_dispatcher.dispatch(
                    WebhookEvent::JobFailed,
                    serde_json::json!({
                        "job_id": job_id.0,
                        "doc_id": doc_id.0,
                        "workspace_id": workspace_id.0,
                        "error": err,
                    }),
                );
            } else {
                jq.mark_completed(&job_id, chunks).await;
                s.webhook_dispatcher.dispatch(
                    WebhookEvent::JobCompleted,
                    serde_json::json!({
                        "job_id": job_id.0,
                        "doc_id": doc_id.0,
                        "workspace_id": workspace_id.0,
                        "chunks_indexed": chunks,
                    }),
                );
            }
        });
        queued += 1;
    }

    // Mark batch job completed (individual sub-jobs track their own status)
    state.job_queue.mark_completed(&batch_job_id, queued).await;

    info!(%workspace_id, count = queued, "Reprocessing all documents in workspace");

    Ok(Json(serde_json::json!({
        "queued": queued,
        "batch_job_id": batch_job_id.0,
        "message": format!("{queued} documents queued for reprocessing")
    })))
}

// ── Batch Upload ─────────────────────────────────────────────────────

/// Maximum number of documents in a single batch upload.
const MAX_BATCH_DOCUMENTS: usize = 500;

/// Maximum size for a single file inside a ZIP archive (10 MB).
const MAX_ZIP_ENTRY_SIZE: usize = 10 * 1024 * 1024;

/// A document extracted from CSV or ZIP, ready for ingestion.
struct BatchEntry {
    title: String,
    content: Vec<u8>,
    mime_type: String,
}

#[derive(Serialize)]
pub struct BatchUploadResponse {
    pub job_id: Uuid,
    pub documents_found: usize,
    pub message: String,
}

/// Accept a CSV or ZIP file and ingest each entry as a separate document.
///
/// **CSV format**: columns `title,content,mime_type` (mime_type optional, defaults to text/plain).
/// **ZIP format**: each file becomes a document; title from filename, mime auto-detected.
pub async fn batch_upload_documents(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<BatchUploadResponse>), ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "batch upload documents")?;

    // Read the uploaded file from the multipart form
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError(ThaiRagError::Validation(format!(
            "Invalid multipart data: {e}"
        )))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            file_name = field.file_name().map(|s| s.to_string());
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
    }

    let bytes = file_bytes
        .ok_or_else(|| ApiError(ThaiRagError::Validation("Missing 'file' field".into())))?;

    // Enforce max batch file size (use same config as single upload)
    let max_bytes = state.config.document.max_upload_size_mb * 1024 * 1024;
    if bytes.len() > max_bytes {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Batch file too large: {} bytes (max {} MB)",
            bytes.len(),
            state.config.document.max_upload_size_mb
        ))));
    }

    // Determine format from file extension
    let extension = file_name
        .as_deref()
        .and_then(|n| n.rsplit('.').next())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let entries = match extension.as_str() {
        "csv" => parse_csv_batch(&bytes)?,
        "zip" => parse_zip_batch(&bytes)?,
        _ => {
            return Err(ApiError(ThaiRagError::Validation(
                "Unsupported batch format. Use .csv or .zip".into(),
            )));
        }
    };

    if entries.is_empty() {
        return Err(ApiError(ThaiRagError::Validation(
            "No documents found in the uploaded file".into(),
        )));
    }

    if entries.len() > MAX_BATCH_DOCUMENTS {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Too many documents: {} (max {MAX_BATCH_DOCUMENTS})",
            entries.len()
        ))));
    }

    // Validate all mime types before starting
    for entry in &entries {
        validate_mime_type(&entry.mime_type)?;
    }

    let doc_count = entries.len();

    info!(
        %workspace_id,
        doc_count,
        format = %extension,
        "Starting batch upload"
    );

    // Create a batch job for progress tracking
    let batch_job = Job {
        id: JobId(Uuid::new_v4()),
        kind: JobKind::BatchUpload,
        status: JobStatus::Queued,
        workspace_id,
        doc_id: None,
        description: format!("Batch upload {doc_count} documents from {extension}"),
        created_at: now_ts(),
        started_at: None,
        completed_at: None,
        error: None,
        items_processed: 0,
        items_total: Some(doc_count),
    };
    let batch_job_id = state.job_queue.enqueue(batch_job).await;

    // Spawn background task to process all entries
    let jq = state.job_queue.clone();
    tokio::spawn(async move {
        jq.mark_running(&batch_job_id).await;

        let mut processed = 0usize;
        let mut failed = 0usize;

        for entry in entries {
            let doc_id = DocId::new();
            let size_bytes = entry.content.len() as i64;
            let now = Utc::now();

            let doc = Document {
                id: doc_id,
                workspace_id,
                title: entry.title.clone(),
                mime_type: entry.mime_type.clone(),
                size_bytes,
                status: DocStatus::Processing,
                chunk_count: 0,
                error_message: None,
                processing_step: None,
                processing_provenance: None,
                processing_timeline: None,
                version: 1,
                content_hash: None,
                source_url: None,
                refresh_schedule: None,
                last_refreshed_at: None,
                created_at: now,
                updated_at: now,
            };

            if let Err(e) = state.km_store.insert_document(doc) {
                warn!(%doc_id, error = %e, "Batch: failed to insert document metadata");
                failed += 1;
                jq.increment_progress(&batch_job_id).await;
                continue;
            }

            let (chunks, error) = process_document_inner(
                state.clone(),
                doc_id,
                workspace_id,
                entry.content,
                entry.mime_type,
            )
            .await;

            if let Some(ref err) = error {
                warn!(%doc_id, %err, "Batch: document processing failed");
                failed += 1;
            } else {
                processed += 1;
                state.webhook_dispatcher.dispatch(
                    WebhookEvent::DocumentIngested,
                    serde_json::json!({
                        "doc_id": doc_id.0,
                        "workspace_id": workspace_id.0,
                        "chunks_indexed": chunks,
                        "batch_job_id": batch_job_id.0,
                    }),
                );
            }

            jq.increment_progress(&batch_job_id).await;
        }

        if failed > 0 && processed == 0 {
            jq.mark_failed(
                &batch_job_id,
                format!("All {failed} documents failed to process"),
            )
            .await;
            state.webhook_dispatcher.dispatch(
                WebhookEvent::JobFailed,
                serde_json::json!({
                    "job_id": batch_job_id.0,
                    "workspace_id": workspace_id.0,
                    "processed": processed,
                    "failed": failed,
                }),
            );
        } else {
            jq.mark_completed(&batch_job_id, processed).await;
            state.webhook_dispatcher.dispatch(
                WebhookEvent::JobCompleted,
                serde_json::json!({
                    "job_id": batch_job_id.0,
                    "workspace_id": workspace_id.0,
                    "processed": processed,
                    "failed": failed,
                }),
            );
        }

        info!(
            %workspace_id,
            processed,
            failed,
            "Batch upload completed"
        );
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(BatchUploadResponse {
            job_id: batch_job_id.0,
            documents_found: doc_count,
            message: "Batch upload started".into(),
        }),
    ))
}

/// Parse a CSV file with columns: title, content, mime_type (optional).
fn parse_csv_batch(bytes: &[u8]) -> Result<Vec<BatchEntry>, ApiError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);

    let headers = rdr.headers().map_err(|e| {
        ApiError(ThaiRagError::Validation(format!(
            "Failed to parse CSV headers: {e}"
        )))
    })?;

    // Find column indices
    let title_idx = headers.iter().position(|h| h.eq_ignore_ascii_case("title"));
    let content_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("content"));

    let title_idx = title_idx.ok_or_else(|| {
        ApiError(ThaiRagError::Validation(
            "CSV missing required 'title' column".into(),
        ))
    })?;
    let content_idx = content_idx.ok_or_else(|| {
        ApiError(ThaiRagError::Validation(
            "CSV missing required 'content' column".into(),
        ))
    })?;
    let mime_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("mime_type"));

    let mut entries = Vec::new();
    for (row_num, result) in rdr.records().enumerate() {
        let record = result.map_err(|e| {
            ApiError(ThaiRagError::Validation(format!(
                "CSV row {}: {e}",
                row_num + 2
            )))
        })?;

        let title = record.get(title_idx).unwrap_or("").trim().to_string();
        let content = record.get(content_idx).unwrap_or("").trim().to_string();

        if title.is_empty() || content.is_empty() {
            continue; // skip empty rows
        }

        let mime_type = mime_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "text/plain".to_string());

        entries.push(BatchEntry {
            title,
            content: content.into_bytes(),
            mime_type,
        });
    }

    Ok(entries)
}

/// Parse a ZIP archive — each file becomes a document.
fn parse_zip_batch(bytes: &[u8]) -> Result<Vec<BatchEntry>, ApiError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
        ApiError(ThaiRagError::Validation(format!(
            "Failed to read ZIP archive: {e}"
        )))
    })?;

    let mut entries = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| {
            ApiError(ThaiRagError::Validation(format!(
                "Failed to read ZIP entry {i}: {e}"
            )))
        })?;

        // Skip directories
        if file.is_dir() {
            continue;
        }

        let name = file.name().to_string();

        // Skip hidden files (starting with . or inside __MACOSX)
        if name.starts_with('.')
            || name.contains("/.")
            || name.starts_with("__MACOSX")
            || name.contains("/__MACOSX")
        {
            continue;
        }

        // Skip files larger than 10MB
        if file.size() > MAX_ZIP_ENTRY_SIZE as u64 {
            warn!(
                file = %name,
                size = file.size(),
                "Skipping ZIP entry: exceeds {MAX_ZIP_ENTRY_SIZE} byte limit"
            );
            continue;
        }

        // Read file contents
        let mut content = Vec::with_capacity(file.size() as usize);
        std::io::Read::read_to_end(&mut file, &mut content).map_err(|e| {
            ApiError(ThaiRagError::Validation(format!(
                "Failed to read ZIP entry '{name}': {e}"
            )))
        })?;

        if content.is_empty() {
            continue;
        }

        // Derive title from filename (strip path prefix)
        let title = name.rsplit('/').next().unwrap_or(&name).to_string();

        // Auto-detect mime type from extension
        let mime_type = name
            .rsplit('.')
            .next()
            .and_then(mime_from_extension)
            .unwrap_or("text/plain")
            .to_string();

        entries.push(BatchEntry {
            title,
            content,
            mime_type,
        });
    }

    Ok(entries)
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
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

// ── Job Queue Handlers ──────────────────────────────────────────────

/// List jobs for a workspace.
pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "list jobs")?;

    let jobs = state.job_queue.list_by_workspace(&workspace_id).await;
    Ok(Json(serde_json::json!({ "jobs": jobs })))
}

/// Get a single job by ID.
pub async fn get_job(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "get job")?;

    let job_id = JobId(job_id);
    let job = state
        .job_queue
        .get(&job_id)
        .await
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Job not found".into())))?;

    // Verify job belongs to the requested workspace
    if job.workspace_id != workspace_id {
        return Err(ApiError(ThaiRagError::NotFound("Job not found".into())));
    }

    Ok(Json(serde_json::json!(job)))
}

/// Cancel a job.
pub async fn cancel_job(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "cancel job")?;

    let job_id = JobId(job_id);

    // Verify job exists and belongs to workspace
    let job = state
        .job_queue
        .get(&job_id)
        .await
        .ok_or_else(|| ApiError(ThaiRagError::NotFound("Job not found".into())))?;
    if job.workspace_id != workspace_id {
        return Err(ApiError(ThaiRagError::NotFound("Job not found".into())));
    }

    let cancelled = state.job_queue.cancel(&job_id).await;
    Ok(Json(serde_json::json!({
        "cancelled": cancelled,
        "job_id": job_id.0,
    })))
}

/// Stream job updates for a workspace via Server-Sent Events.
/// Polls the job queue every 2 seconds and sends the full job list
/// as an SSE event whenever it changes (or periodically as a heartbeat).
pub async fn stream_jobs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "stream jobs")?;

    let stream = async_stream::stream! {
        let mut last_hash: u64 = 0;
        loop {
            let jobs = state.job_queue.list_by_workspace(&workspace_id).await;
            let payload = serde_json::json!({ "jobs": jobs });

            // Only send when data actually changed (content hash comparison)
            let current_hash = {
                use std::hash::{Hash, Hasher};
                let json = serde_json::to_string(&payload).unwrap_or_default();
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                json.hash(&mut hasher);
                hasher.finish()
            };

            if current_hash != last_hash {
                last_hash = current_hash;
                let data = serde_json::to_string(&payload).unwrap_or_default();
                yield Ok::<_, std::convert::Infallible>(
                    Event::default().event("jobs").data(data)
                );
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    ))
}

// ── Document Versioning Handlers ────────────────────────────────────

use crate::store::{DiffStats, DocumentVersion};

/// List all versions for a document.
pub async fn list_document_versions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "list document versions")?;

    let doc_id = DocId(doc_id);
    let _ = state.km_store.get_document(doc_id)?;

    let versions = state.km_store.list_document_versions(doc_id);
    Ok(Json(serde_json::json!({
        "doc_id": doc_id.0,
        "versions": versions,
        "total": versions.len(),
    })))
}

/// Get a specific version of a document.
pub async fn get_document_version(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id, version)): Path<(Uuid, Uuid, i32)>,
) -> Result<Json<DocumentVersion>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "read document version")?;

    let doc_id = DocId(doc_id);
    let _ = state.km_store.get_document(doc_id)?;

    state
        .km_store
        .get_document_version(doc_id, version)
        .map(Json)
        .ok_or_else(|| {
            ApiError(ThaiRagError::NotFound(format!(
                "Version {version} not found for document {doc_id}"
            )))
        })
}

/// Diff between two versions -- returns line-level addition/deletion stats.
#[derive(Deserialize)]
pub struct DiffQuery {
    pub from: i32,
    pub to: i32,
}

pub async fn diff_document_versions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<DiffStats>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_read, "diff document versions")?;

    let doc_id = DocId(doc_id);
    let _ = state.km_store.get_document(doc_id)?;

    // Resolve "from" content -- version 0 means empty (for diffing against v1)
    let from_content = if query.from == 0 {
        String::new()
    } else {
        let v = state
            .km_store
            .get_document_version(doc_id, query.from)
            .ok_or_else(|| {
                ApiError(ThaiRagError::NotFound(format!(
                    "Version {} not found for document {doc_id}",
                    query.from
                )))
            })?;
        v.content.unwrap_or_default()
    };

    // Resolve "to" content -- if version not found in history, use current doc content
    let to_content = if let Some(v) = state.km_store.get_document_version(doc_id, query.to) {
        v.content.unwrap_or_default()
    } else {
        // "to" is the current version -- get from document_blobs
        state
            .km_store
            .get_document_content(doc_id)
            .unwrap_or(None)
            .unwrap_or_default()
    };

    // Simple line-by-line diff stats
    let from_lines: Vec<&str> = from_content.lines().collect();
    let to_lines: Vec<&str> = to_content.lines().collect();

    let mut additions = 0usize;
    let mut deletions = 0usize;

    // Build multisets of lines for simple comparison
    let mut from_set = std::collections::HashMap::<&str, usize>::new();
    for line in &from_lines {
        *from_set.entry(line).or_insert(0) += 1;
    }
    let mut to_set = std::collections::HashMap::<&str, usize>::new();
    for line in &to_lines {
        *to_set.entry(line).or_insert(0) += 1;
    }

    for (line, count) in &from_set {
        let to_count = to_set.get(line).copied().unwrap_or(0);
        if *count > to_count {
            deletions += count - to_count;
        }
    }
    for (line, count) in &to_set {
        let from_count = from_set.get(line).copied().unwrap_or(0);
        if *count > from_count {
            additions += count - from_count;
        }
    }

    Ok(Json(DiffStats {
        from_version: query.from,
        to_version: query.to,
        additions,
        deletions,
    }))
}

/// Helper: compute SHA-256 hash of content bytes.
pub fn compute_content_hash(content: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Save the current document state as a version before overwriting.
/// Called automatically during reprocessing / re-ingestion.
pub fn save_current_version(state: &AppState, doc_id: DocId, created_by: Option<UserId>) {
    let doc = match state.km_store.get_document(doc_id) {
        Ok(d) => d,
        Err(_) => return,
    };

    // Get the current converted text for the version snapshot
    let content = state.km_store.get_document_content(doc_id).unwrap_or(None);

    let content_hash = doc
        .content_hash
        .clone()
        .unwrap_or_else(|| compute_content_hash(content.as_deref().unwrap_or("").as_bytes()));

    match state.km_store.save_document_version(
        doc_id,
        &doc.title,
        content.as_deref(),
        &content_hash,
        &doc.mime_type,
        doc.size_bytes,
        created_by,
    ) {
        Ok(ver) => {
            info!(%doc_id, version = ver.version_number, "Saved document version before update");
        }
        Err(e) => {
            warn!(%doc_id, error = %e, "Failed to save document version (non-fatal)");
        }
    }
}

// ── Document Refresh Schedule ────────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateScheduleRequest {
    /// Source URL to fetch content from.
    pub source_url: Option<String>,
    /// Refresh interval (e.g., "1h", "6h", "1d", "7d", "30d"). Set to null to clear.
    pub refresh_schedule: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateScheduleResponse {
    pub doc_id: Uuid,
    pub source_url: Option<String>,
    pub refresh_schedule: Option<String>,
}

pub async fn update_document_schedule(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((workspace_id, doc_id)): Path<(Uuid, Uuid)>,
    AppJson(body): AppJson<UpdateScheduleRequest>,
) -> Result<Json<UpdateScheduleResponse>, ApiError> {
    let workspace_id = WorkspaceId(workspace_id);
    let perm = resolve_doc_perm(&claims, &state, workspace_id)?;
    require_doc(&perm, Role::can_write, "update document schedule")?;

    let doc_id_typed = DocId(doc_id);

    // Verify document exists
    let _ = state.km_store.get_document(doc_id_typed)?;

    // Validate refresh_schedule if provided
    if let Some(ref schedule) = body.refresh_schedule
        && !crate::store::is_valid_refresh_schedule(schedule)
    {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "Invalid refresh_schedule: '{schedule}'. Use formats like '1h', '6h', '1d', '7d', '30d'"
        ))));
    }

    // Validate source_url if refresh_schedule is set
    if body.refresh_schedule.is_some() && body.source_url.is_none() {
        let doc = state.km_store.get_document(doc_id_typed)?;
        if doc.source_url.is_none() {
            return Err(ApiError(ThaiRagError::Validation(
                "source_url is required when setting refresh_schedule".into(),
            )));
        }
    }

    state.km_store.update_document_schedule(
        doc_id_typed,
        body.source_url.clone(),
        body.refresh_schedule.clone(),
    )?;

    info!(
        %doc_id, source_url = ?body.source_url, schedule = ?body.refresh_schedule,
        "Updated document refresh schedule"
    );

    Ok(Json(UpdateScheduleResponse {
        doc_id,
        source_url: body.source_url,
        refresh_schedule: body.refresh_schedule,
    }))
}

// ── Background Document Refresh Scheduler ────────────────────────────

/// Spawns a background task that periodically checks for documents due for refresh
/// and re-ingests them from their source URL.
pub fn spawn_document_refresh_scheduler(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        interval.tick().await; // first tick fires immediately

        loop {
            interval.tick().await;

            let due_docs = state.km_store.list_documents_due_for_refresh();
            if due_docs.is_empty() {
                continue;
            }

            tracing::info!(count = due_docs.len(), "Documents due for refresh");

            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(3));

            for doc in due_docs {
                let state = state.clone();
                let sem = semaphore.clone();
                tokio::spawn(async move {
                    let _permit = match sem.acquire().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    refresh_document_from_source(state, doc).await;
                });
            }
        }
    });
}

async fn refresh_document_from_source(state: AppState, doc: Document) {
    let doc_id = doc.id;
    let workspace_id = doc.workspace_id;
    let source_url = match &doc.source_url {
        Some(url) => url.clone(),
        None => return,
    };

    tracing::info!(%doc_id, %source_url, "Refreshing document from source URL");

    let job = thairag_core::types::Job {
        id: thairag_core::types::JobId(Uuid::new_v4()),
        kind: thairag_core::types::JobKind::DocumentRefresh,
        status: thairag_core::types::JobStatus::Queued,
        workspace_id,
        doc_id: Some(doc_id),
        description: format!("Refresh document {doc_id} from {source_url}"),
        created_at: now_ts(),
        started_at: None,
        completed_at: None,
        error: None,
        items_processed: 0,
        items_total: None,
    };
    let job_id = state.job_queue.enqueue(job).await;
    let jq = state.job_queue.clone();
    jq.mark_running(&job_id).await;

    // Fetch content from source URL
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_default();

    let response = match client.get(&source_url).send().await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Failed to fetch from {source_url}: {e}");
            tracing::error!(%doc_id, %msg);
            jq.mark_failed(&job_id, msg).await;
            return;
        }
    };

    if !response.status().is_success() {
        let msg = format!("HTTP {} from {source_url}", response.status());
        tracing::error!(%doc_id, %msg);
        jq.mark_failed(&job_id, msg).await;
        return;
    }

    let bytes = match response.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            let msg = format!("Failed to read response body from {source_url}: {e}");
            tracing::error!(%doc_id, %msg);
            jq.mark_failed(&job_id, msg).await;
            return;
        }
    };

    if bytes.is_empty() {
        let msg = "Empty response from source URL".to_string();
        tracing::warn!(%doc_id, %msg);
        jq.mark_failed(&job_id, msg).await;
        return;
    }

    // Preserve the current bytes + chunks as a version snapshot before
    // we overwrite them with the fetched content. Otherwise the user's
    // original upload is lost the first time a scheduled refresh fires.
    save_current_version(&state, doc_id, None);

    // Delete old chunks from search index AND SQL store. See
    // single-doc reprocess for why both deletes are required.
    let _ = state.providers().search_engine.delete_doc(doc_id).await;
    let _ = state.km_store.delete_chunks_by_doc(doc_id);

    // Mark as processing
    let _ = state
        .km_store
        .update_document_status(doc_id, DocStatus::Processing, 0, None);

    // Reprocess with the new content
    let mime = doc.mime_type.clone();
    let (chunks, error) =
        process_document_inner(state.clone(), doc_id, workspace_id, bytes, mime).await;

    if let Some(ref err) = error {
        jq.mark_failed(&job_id, err.clone()).await;
        state.webhook_dispatcher.dispatch(
            WebhookEvent::JobFailed,
            serde_json::json!({
                "job_id": job_id.0,
                "doc_id": doc_id.0,
                "workspace_id": workspace_id.0,
                "error": err,
                "kind": "document_refresh",
            }),
        );
    } else {
        jq.mark_completed(&job_id, chunks).await;
        let _ = state.km_store.touch_document_refreshed(doc_id);

        tracing::info!(%doc_id, chunks, "Document refreshed successfully from source URL");
        state.webhook_dispatcher.dispatch(
            WebhookEvent::DocumentIngested,
            serde_json::json!({
                "doc_id": doc_id.0,
                "workspace_id": workspace_id.0,
                "chunks_indexed": chunks,
                "kind": "document_refresh",
                "source_url": source_url,
            }),
        );
    }
}

#[cfg(test)]
mod panic_guard_tests {
    use super::*;

    #[test]
    fn panic_payload_to_string_extracts_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("boom"));
        assert_eq!(panic_payload_to_string(payload), "boom");
    }

    #[test]
    fn panic_payload_to_string_extracts_static_str_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("static panic");
        assert_eq!(panic_payload_to_string(payload), "static panic");
    }

    #[test]
    fn panic_payload_to_string_falls_back_for_unknown_payload() {
        // panic!(42) produces an i32 payload — we cannot interpret it but
        // we MUST NOT lose the "this was a panic" signal.
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        let out = panic_payload_to_string(payload);
        assert!(!out.is_empty());
        assert!(
            out.contains("panic") || out.contains("unknown"),
            "fallback must hint at panic, got: {out}"
        );
    }

    #[tokio::test]
    async fn child_task_panic_is_observable_as_join_error_is_panic() {
        // Regression: confirms the tokio semantics this PR's guard relies on.
        // If this test ever fails, the panic-guard in `process_document_inner`
        // is no longer protecting anything and needs a different approach.
        let handle = tokio::spawn(async {
            panic!("intentional test panic");
        });
        let err = handle
            .await
            .expect_err("child task panic must surface as JoinError");
        assert!(err.is_panic(), "JoinError must report is_panic() == true");
        let msg = panic_payload_to_string(err.into_panic());
        assert_eq!(msg, "intentional test panic");
    }
}
