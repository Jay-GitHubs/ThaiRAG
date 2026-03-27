use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{DocumentAnnotation, DocumentComment, DocumentReview};

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateCommentRequest {
    pub user_id: String,
    pub user_name: Option<String>,
    pub text: String,
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateAnnotationRequest {
    pub user_id: String,
    pub user_name: Option<String>,
    pub chunk_id: Option<String>,
    pub text: String,
    pub highlight_start: Option<u32>,
    pub highlight_end: Option<u32>,
}

#[derive(Deserialize)]
pub struct CreateReviewRequest {
    pub reviewer_id: String,
    pub reviewer_name: Option<String>,
    pub status: Option<String>,
    pub comments: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateReviewStatusRequest {
    pub status: String,
    pub comments: Option<String>,
}

#[derive(Serialize)]
pub struct ListResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: usize,
}

// ── Comment Handlers ─────────────────────────────────────────────────

pub async fn create_comment(
    State(state): State<AppState>,
    Path((_ws_id, doc_id)): Path<(String, String)>,
    AppJson(body): AppJson<CreateCommentRequest>,
) -> Result<(StatusCode, Json<DocumentComment>), ApiError> {
    let comment = DocumentComment {
        id: uuid::Uuid::new_v4().to_string(),
        doc_id,
        user_id: body.user_id,
        user_name: body.user_name,
        text: body.text,
        parent_id: body.parent_id,
        created_at: Utc::now().to_rfc3339(),
    };
    let created = state.km_store.insert_comment(&comment)?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_comments(
    State(state): State<AppState>,
    Path((_ws_id, doc_id)): Path<(String, String)>,
) -> Json<ListResponse<DocumentComment>> {
    let data = state.km_store.list_comments(&doc_id);
    let total = data.len();
    Json(ListResponse { data, total })
}

pub async fn delete_comment(
    State(state): State<AppState>,
    Path((_ws_id, _doc_id, comment_id)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    state.km_store.delete_comment(&comment_id)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Annotation Handlers ──────────────────────────────────────────────

pub async fn create_annotation(
    State(state): State<AppState>,
    Path((_ws_id, doc_id)): Path<(String, String)>,
    AppJson(body): AppJson<CreateAnnotationRequest>,
) -> Result<(StatusCode, Json<DocumentAnnotation>), ApiError> {
    let annotation = DocumentAnnotation {
        id: uuid::Uuid::new_v4().to_string(),
        doc_id,
        user_id: body.user_id,
        user_name: body.user_name,
        chunk_id: body.chunk_id,
        text: body.text,
        highlight_start: body.highlight_start,
        highlight_end: body.highlight_end,
        created_at: Utc::now().to_rfc3339(),
    };
    let created = state.km_store.insert_annotation(&annotation)?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_annotations(
    State(state): State<AppState>,
    Path((_ws_id, doc_id)): Path<(String, String)>,
) -> Json<ListResponse<DocumentAnnotation>> {
    let data = state.km_store.list_annotations(&doc_id);
    let total = data.len();
    Json(ListResponse { data, total })
}

pub async fn delete_annotation(
    State(state): State<AppState>,
    Path((_ws_id, _doc_id, annotation_id)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    state.km_store.delete_annotation(&annotation_id)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Review Handlers ──────────────────────────────────────────────────

pub async fn create_review(
    State(state): State<AppState>,
    Path((_ws_id, doc_id)): Path<(String, String)>,
    AppJson(body): AppJson<CreateReviewRequest>,
) -> Result<(StatusCode, Json<DocumentReview>), ApiError> {
    let now = Utc::now().to_rfc3339();
    let review = DocumentReview {
        id: uuid::Uuid::new_v4().to_string(),
        doc_id,
        reviewer_id: body.reviewer_id,
        reviewer_name: body.reviewer_name,
        status: body.status.unwrap_or_else(|| "pending".to_string()),
        comments: body.comments,
        created_at: now.clone(),
        updated_at: now,
    };
    let created = state.km_store.insert_review(&review)?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn list_reviews(
    State(state): State<AppState>,
    Path((_ws_id, doc_id)): Path<(String, String)>,
) -> Json<ListResponse<DocumentReview>> {
    let data = state.km_store.list_reviews(&doc_id);
    let total = data.len();
    Json(ListResponse { data, total })
}

pub async fn update_review_status(
    State(state): State<AppState>,
    Path((_ws_id, _doc_id, review_id)): Path<(String, String, String)>,
    AppJson(body): AppJson<UpdateReviewStatusRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .km_store
        .update_review_status(&review_id, &body.status, body.comments.as_deref())?;
    Ok(StatusCode::NO_CONTENT)
}
