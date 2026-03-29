use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{FinetuneJob, TrainingDataset, TrainingPair};
use thairag_core::ThaiRagError;

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDatasetRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct AddPairRequest {
    pub query: String,
    pub positive_doc: String,
    pub negative_doc: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub dataset_id: String,
    pub base_model: String,
}

#[derive(Serialize)]
pub struct ListResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: usize,
}

// ── Validation ────────────────────────────────────────────────────────

fn validate_nonempty(value: &str, field: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Err(ApiError(ThaiRagError::Validation(format!(
            "'{field}' must not be empty"
        ))));
    }
    Ok(())
}

// ── Dataset Handlers ─────────────────────────────────────────────────

/// GET /api/km/finetune/datasets
pub async fn list_datasets(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<TrainingDataset>>, ApiError> {
    let datasets = state.km_store.list_training_datasets();
    let total = datasets.len();
    Ok(Json(ListResponse {
        data: datasets,
        total,
    }))
}

/// POST /api/km/finetune/datasets
pub async fn create_dataset(
    State(state): State<AppState>,
    AppJson(body): AppJson<CreateDatasetRequest>,
) -> Result<(StatusCode, Json<TrainingDataset>), ApiError> {
    validate_nonempty(&body.name, "name")?;
    let description = body.description.unwrap_or_default();
    let ds = state
        .km_store
        .insert_training_dataset(body.name, description)
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(ds)))
}

/// GET /api/km/finetune/datasets/{id}
pub async fn get_dataset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrainingDataset>, ApiError> {
    let ds = state
        .km_store
        .get_training_dataset(&id)
        .map_err(ApiError::from)?;
    Ok(Json(ds))
}

/// DELETE /api/km/finetune/datasets/{id}
pub async fn delete_dataset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .km_store
        .delete_training_dataset(&id)
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Pair Handlers ─────────────────────────────────────────────────────

/// GET /api/km/finetune/datasets/{id}/pairs
pub async fn list_pairs(
    State(state): State<AppState>,
    Path(dataset_id): Path<String>,
) -> Result<Json<ListResponse<TrainingPair>>, ApiError> {
    let pairs = state.km_store.list_training_pairs(&dataset_id);
    let total = pairs.len();
    Ok(Json(ListResponse { data: pairs, total }))
}

/// POST /api/km/finetune/datasets/{id}/pairs
pub async fn add_pair(
    State(state): State<AppState>,
    Path(dataset_id): Path<String>,
    AppJson(body): AppJson<AddPairRequest>,
) -> Result<(StatusCode, Json<TrainingPair>), ApiError> {
    validate_nonempty(&body.query, "query")?;
    validate_nonempty(&body.positive_doc, "positive_doc")?;
    let pair = TrainingPair {
        id: Uuid::new_v4().to_string(),
        dataset_id,
        query: body.query,
        positive_doc: body.positive_doc,
        negative_doc: body.negative_doc,
        created_at: Utc::now().to_rfc3339(),
    };
    let pair = state
        .km_store
        .insert_training_pair(&pair)
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(pair)))
}

/// DELETE /api/km/finetune/datasets/{id}/pairs/{pair_id}
pub async fn delete_pair(
    State(state): State<AppState>,
    Path((_dataset_id, pair_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state
        .km_store
        .delete_training_pair(&pair_id)
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Job Handlers ──────────────────────────────────────────────────────

/// GET /api/km/finetune/jobs
pub async fn list_jobs(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<FinetuneJob>>, ApiError> {
    let jobs = state.km_store.list_finetune_jobs();
    let total = jobs.len();
    Ok(Json(ListResponse { data: jobs, total }))
}

/// POST /api/km/finetune/jobs
pub async fn create_job(
    State(state): State<AppState>,
    AppJson(body): AppJson<CreateJobRequest>,
) -> Result<(StatusCode, Json<FinetuneJob>), ApiError> {
    validate_nonempty(&body.dataset_id, "dataset_id")?;
    validate_nonempty(&body.base_model, "base_model")?;
    // Verify dataset exists
    state
        .km_store
        .get_training_dataset(&body.dataset_id)
        .map_err(|_| {
            ApiError(thairag_core::ThaiRagError::NotFound(format!(
                "Dataset {} not found",
                body.dataset_id
            )))
        })?;

    let now = Utc::now().to_rfc3339();
    let job = FinetuneJob {
        id: Uuid::new_v4().to_string(),
        dataset_id: body.dataset_id,
        base_model: body.base_model,
        status: "pending".to_string(),
        metrics: None,
        output_model_path: None,
        created_at: now.clone(),
        updated_at: now,
    };
    let job = state
        .km_store
        .insert_finetune_job(&job)
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(job)))
}

/// GET /api/km/finetune/jobs/{id}
pub async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<FinetuneJob>, ApiError> {
    let job = state
        .km_store
        .get_finetune_job(&id)
        .map_err(ApiError::from)?;
    Ok(Json(job))
}
