use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::BackupIncludes;

use crate::app_state::AppState;
use crate::backup::{self, BackupPreview, RestoreOptions, RestoreResult};
use crate::error::ApiError;
use crate::routes::settings::require_super_admin;

// ── Request/Response Types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateBackupRequest {
    #[serde(default = "default_true")]
    pub include_settings: bool,
    #[serde(default = "default_true")]
    pub include_users: bool,
    #[serde(default = "default_true")]
    pub include_documents: bool,
    #[serde(default = "default_true")]
    pub include_org_structure: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct RestoreQuery {
    #[serde(default)]
    pub skip_existing: bool,
}

// ── POST /api/admin/backup — create backup (returns ZIP) ────────────

pub async fn create_backup(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateBackupRequest>,
) -> Result<Response, ApiError> {
    require_super_admin(&claims, &state)?;

    let includes = BackupIncludes {
        settings: req.include_settings,
        users: req.include_users,
        documents: req.include_documents,
        org_structure: req.include_org_structure,
    };

    let zip_bytes = tokio::task::spawn_blocking({
        let store = state.km_store.clone();
        move || backup::create_backup(store.as_ref(), &includes)
    })
    .await
    .map_err(|e| ApiError(ThaiRagError::Internal(format!("Task join error: {e}"))))??;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("thairag-backup-{timestamp}.zip");

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{filename}\"")
            .parse()
            .unwrap(),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        zip_bytes.len().to_string().parse().unwrap(),
    );

    Ok((StatusCode::OK, headers, Body::from(zip_bytes)).into_response())
}

// ── POST /api/admin/restore — restore from backup (multipart) ──────

pub async fn restore_backup(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(query): Query<RestoreQuery>,
    mut multipart: Multipart,
) -> Result<Json<RestoreResult>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Read file from multipart
    let mut file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError(ThaiRagError::Validation(format!(
            "Invalid multipart data: {e}"
        )))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
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

    let options = RestoreOptions {
        settings: true,
        users: true,
        org_structure: true,
        skip_existing: query.skip_existing,
    };

    let result = tokio::task::spawn_blocking({
        let store = state.km_store.clone();
        move || backup::restore_backup(store, &bytes, &options)
    })
    .await
    .map_err(|e| ApiError(ThaiRagError::Internal(format!("Task join error: {e}"))))??;

    Ok(Json(result))
}

// ── POST /api/admin/backup/preview — dry-run restore ────────────────

pub async fn preview_backup(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    mut multipart: Multipart,
) -> Result<Json<BackupPreview>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Read file from multipart
    let mut file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError(ThaiRagError::Validation(format!(
            "Invalid multipart data: {e}"
        )))
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
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

    let preview = tokio::task::spawn_blocking(move || backup::preview_backup(&bytes))
        .await
        .map_err(|e| ApiError(ThaiRagError::Internal(format!("Task join error: {e}"))))??;

    Ok(Json(preview))
}
