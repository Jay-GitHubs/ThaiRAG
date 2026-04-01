use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::routes::feedback::{FeedbackEntry, GoldenExample};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingConfig {
    #[serde(default = "default_epochs")]
    pub epochs: u32,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f64,
    #[serde(default = "default_lora_rank")]
    pub lora_rank: u32,
    #[serde(default = "default_lora_alpha")]
    pub lora_alpha: u32,
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_warmup_ratio")]
    pub warmup_ratio: f64,
    #[serde(default = "default_max_seq_length")]
    pub max_seq_length: u32,
    #[serde(default = "default_quantization")]
    pub quantization: String,
    pub preset: Option<String>,
}

fn default_epochs() -> u32 {
    3
}
fn default_learning_rate() -> f64 {
    2e-4
}
fn default_lora_rank() -> u32 {
    16
}
fn default_lora_alpha() -> u32 {
    16
}
fn default_batch_size() -> u32 {
    2
}
fn default_warmup_ratio() -> f64 {
    0.03
}
fn default_max_seq_length() -> u32 {
    2048
}
fn default_quantization() -> String {
    "q4_k_m".into()
}

impl TrainingConfig {
    /// Apply preset values, overriding defaults.
    pub fn apply_preset(mut self) -> Self {
        match self.preset.as_deref() {
            Some("quick") => {
                self.epochs = 1;
                self.learning_rate = 5e-4;
                self.lora_rank = 8;
                self.lora_alpha = 8;
                self.batch_size = 4;
            }
            Some("standard") => {
                self.epochs = 3;
                self.learning_rate = 2e-4;
                self.lora_rank = 16;
                self.lora_alpha = 16;
                self.batch_size = 2;
            }
            Some("thorough") => {
                self.epochs = 5;
                self.learning_rate = 1e-4;
                self.lora_rank = 32;
                self.lora_alpha = 32;
                self.batch_size = 2;
            }
            _ => {}
        }
        self
    }
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            epochs: default_epochs(),
            learning_rate: default_learning_rate(),
            lora_rank: default_lora_rank(),
            lora_alpha: default_lora_alpha(),
            batch_size: default_batch_size(),
            warmup_ratio: default_warmup_ratio(),
            max_seq_length: default_max_seq_length(),
            quantization: default_quantization(),
            preset: None,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub dataset_id: String,
    pub base_model: String,
    pub model_source: Option<String>,
    pub config: Option<TrainingConfig>,
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

    // Build config with preset applied
    let config = body.config.unwrap_or_default().apply_preset();
    let mut config_json = serde_json::to_value(&config).unwrap_or_default();
    // Include model_source in config JSON for the runner
    if let Some(ref source) = body.model_source {
        config_json["model_source"] = serde_json::Value::String(source.clone());
    }

    let now = Utc::now().to_rfc3339();
    let job = FinetuneJob {
        id: Uuid::new_v4().to_string(),
        dataset_id: body.dataset_id,
        base_model: body.base_model,
        status: "pending".to_string(),
        metrics: None,
        output_model_path: None,
        config: Some(serde_json::to_string(&config_json).unwrap_or_default()),
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

/// POST /api/km/finetune/jobs/{id}/start
pub async fn start_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .training_runner
        .start_training(&state, &id)
        .await
        .map_err(|e| ApiError(ThaiRagError::Internal(e)))?;
    Ok(Json(
        serde_json::json!({ "status": "started", "job_id": id }),
    ))
}

/// POST /api/km/finetune/jobs/{id}/cancel
pub async fn cancel_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .training_runner
        .cancel_training(&id)
        .map_err(|e| ApiError(ThaiRagError::Internal(e)))?;
    // Also update store status
    let _ = state
        .km_store
        .update_finetune_job_status(&id, "cancelled", None);
    Ok(Json(
        serde_json::json!({ "status": "cancelled", "job_id": id }),
    ))
}

/// GET /api/km/finetune/jobs/{id}/logs
pub async fn get_job_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Verify job exists
    state
        .km_store
        .get_finetune_job(&id)
        .map_err(ApiError::from)?;
    let logs = state.training_runner.get_logs(&id);
    Ok(Json(serde_json::json!({ "job_id": id, "lines": logs })))
}

/// DELETE /api/km/finetune/jobs/{id}
pub async fn delete_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Cannot delete running jobs
    if state.training_runner.is_running(&id) {
        return Err(ApiError(ThaiRagError::Validation(
            "Cannot delete a running job. Cancel it first.".into(),
        )));
    }
    state
        .km_store
        .delete_finetune_job(&id)
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Import Feedback → Training Pairs ────────────────────────────────

#[derive(Deserialize)]
pub struct ImportFeedbackRequest {
    /// "positive_feedback", "golden_examples", or "both"
    pub source: String,
    /// Min average chunk score filter (optional)
    pub min_score: Option<f32>,
    /// Filter by workspace (optional)
    pub workspace_id: Option<String>,
}

#[derive(Serialize)]
pub struct ImportFeedbackResponse {
    pub imported: usize,
    pub skipped_duplicates: usize,
}

/// POST /api/km/finetune/datasets/{id}/import-feedback
pub async fn import_feedback(
    State(state): State<AppState>,
    Path(dataset_id): Path<String>,
    AppJson(body): AppJson<ImportFeedbackRequest>,
) -> Result<Json<ImportFeedbackResponse>, ApiError> {
    // Verify dataset exists
    state
        .km_store
        .get_training_dataset(&dataset_id)
        .map_err(ApiError::from)?;

    // Load existing pairs for dedup
    let existing_pairs = state.km_store.list_training_pairs(&dataset_id);
    let existing_set: std::collections::HashSet<(String, String)> = existing_pairs
        .iter()
        .map(|p| (p.query.clone(), p.positive_doc.clone()))
        .collect();

    let include_feedback = body.source == "positive_feedback" || body.source == "both";
    let include_golden = body.source == "golden_examples" || body.source == "both";

    let mut candidates: Vec<(String, String)> = Vec::new();

    // Collect from positive feedback
    if include_feedback {
        let entries: Vec<FeedbackEntry> = state
            .km_store
            .get_setting("feedback:log")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        for entry in &entries {
            if !entry.thumbs_up {
                continue;
            }
            let (Some(query), Some(answer)) = (&entry.query, &entry.answer) else {
                continue;
            };
            if query.trim().is_empty() || answer.trim().is_empty() {
                continue;
            }
            // Optional workspace filter
            if let Some(ref ws) = body.workspace_id
                && entry.workspace_id.as_deref() != Some(ws.as_str())
            {
                continue;
            }
            // Optional min_score filter
            if let Some(min) = body.min_score
                && !entry.chunk_scores.is_empty()
            {
                let avg = entry.chunk_scores.iter().sum::<f32>() / entry.chunk_scores.len() as f32;
                if avg < min {
                    continue;
                }
            }
            candidates.push((query.clone(), answer.clone()));
        }
    }

    // Collect from golden examples
    if include_golden {
        let examples: Vec<GoldenExample> = state
            .km_store
            .get_setting("feedback:golden_examples")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        for ex in &examples {
            if ex.query.trim().is_empty() || ex.answer.trim().is_empty() {
                continue;
            }
            if let Some(ref ws) = body.workspace_id
                && ex.workspace_id.as_deref() != Some(ws.as_str())
            {
                continue;
            }
            candidates.push((ex.query.clone(), ex.answer.clone()));
        }
    }

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for (query, positive_doc) in candidates {
        if existing_set.contains(&(query.clone(), positive_doc.clone())) {
            skipped += 1;
            continue;
        }
        let pair = TrainingPair {
            id: Uuid::new_v4().to_string(),
            dataset_id: dataset_id.clone(),
            query,
            positive_doc,
            negative_doc: None,
            created_at: Utc::now().to_rfc3339(),
        };
        if state.km_store.insert_training_pair(&pair).is_ok() {
            imported += 1;
        }
    }

    Ok(Json(ImportFeedbackResponse {
        imported,
        skipped_duplicates: skipped,
    }))
}

// ── Export Dataset as JSONL ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExportQuery {
    /// "openai" or "alpaca"
    #[serde(default = "default_export_format")]
    pub format: String,
}

fn default_export_format() -> String {
    "openai".to_string()
}

/// GET /api/km/finetune/datasets/{id}/export?format=openai|alpaca
pub async fn export_dataset(
    State(state): State<AppState>,
    Path(dataset_id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    // Verify dataset exists
    let ds = state
        .km_store
        .get_training_dataset(&dataset_id)
        .map_err(ApiError::from)?;

    let pairs = state.km_store.list_training_pairs(&dataset_id);
    let format = q.format.as_str();

    let mut lines: Vec<String> = Vec::with_capacity(pairs.len());

    for pair in &pairs {
        match format {
            "alpaca" => {
                let entry = serde_json::json!({
                    "instruction": pair.query,
                    "input": "",
                    "output": pair.positive_doc,
                });
                lines.push(serde_json::to_string(&entry).unwrap_or_default());
                // If negative_doc present, add rejected entry for DPO
                if let Some(ref neg) = pair.negative_doc {
                    let rejected = serde_json::json!({
                        "instruction": pair.query,
                        "input": "",
                        "output": neg,
                        "rejected": true,
                    });
                    lines.push(serde_json::to_string(&rejected).unwrap_or_default());
                }
            }
            _ => {
                // OpenAI chat format
                let entry = serde_json::json!({
                    "messages": [
                        {"role": "system", "content": "You are a helpful Thai RAG assistant."},
                        {"role": "user", "content": pair.query},
                        {"role": "assistant", "content": pair.positive_doc},
                    ]
                });
                lines.push(serde_json::to_string(&entry).unwrap_or_default());
                if let Some(ref neg) = pair.negative_doc {
                    let rejected = serde_json::json!({
                        "messages": [
                            {"role": "system", "content": "You are a helpful Thai RAG assistant."},
                            {"role": "user", "content": pair.query},
                            {"role": "assistant", "content": neg},
                        ],
                        "rejected": true,
                    });
                    lines.push(serde_json::to_string(&rejected).unwrap_or_default());
                }
            }
        }
    }

    let body_str = lines.join("\n");
    let filename = format!("{}-{}.jsonl", ds.name.replace(' ', "_"), format);

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "application/jsonl")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from(body_str))
        .unwrap())
}
