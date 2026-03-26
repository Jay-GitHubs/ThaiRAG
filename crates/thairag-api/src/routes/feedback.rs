use std::collections::HashMap;
use std::sync::atomic::Ordering;

use axum::extract::{Query, State};
use axum::{Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thairag_auth::AuthClaims;
use tracing::{debug, info};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};

/// Maximum feedback entries kept in the log.
const MAX_FEEDBACK_ENTRIES: usize = 5000;

// ── Feedback Entry ──────────────────────────────────────────────────

/// A rich feedback entry stored per-response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    pub response_id: String,
    pub user_id: String,
    pub thumbs_up: bool,
    #[serde(default)]
    pub comment: Option<String>,
    pub timestamp: i64,
    // Rich context for auto-tuning
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Doc IDs of chunks that were retrieved (for document boost/penalty).
    #[serde(default)]
    pub doc_ids: Vec<String>,
    /// Chunk scores that were retrieved (parallel to doc_ids).
    #[serde(default)]
    pub chunk_scores: Vec<f32>,
    /// Chunk IDs that were retrieved.
    #[serde(default)]
    pub chunk_ids: Vec<String>,
}

// ── Request / Response types ────────────────────────────────────────

#[derive(Deserialize)]
pub struct FeedbackRequest {
    pub response_id: String,
    pub thumbs_up: bool,
    #[serde(default)]
    pub comment: Option<String>,
    // Optional rich context (sent from Test Chat)
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub doc_ids: Vec<String>,
    #[serde(default)]
    pub chunk_scores: Vec<f32>,
    #[serde(default)]
    pub chunk_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct FeedbackResponse {
    pub ok: bool,
}

/// POST /v1/chat/feedback — submit feedback for a response.
pub async fn submit_feedback(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, ApiError> {
    let entry = FeedbackEntry {
        response_id: req.response_id.clone(),
        user_id: claims.sub.clone(),
        thumbs_up: req.thumbs_up,
        comment: req.comment,
        timestamp: Utc::now().timestamp(),
        query: req.query,
        answer: req.answer,
        workspace_id: req.workspace_id,
        doc_ids: req.doc_ids,
        chunk_scores: req.chunk_scores,
        chunk_ids: req.chunk_ids,
    };

    // Append to the feedback log
    let key = "feedback:log";
    let mut entries: Vec<FeedbackEntry> = state
        .km_store
        .get_setting(key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    entries.push(entry);

    // Keep last N entries
    if entries.len() > MAX_FEEDBACK_ENTRIES {
        entries.drain(..entries.len() - MAX_FEEDBACK_ENTRIES);
    }

    if let Ok(json) = serde_json::to_string(&entries) {
        state.km_store.set_setting(key, &json);
    }

    debug!(response_id = %req.response_id, thumbs_up = req.thumbs_up, "Feedback received");

    // Recompute adaptive threshold if we have enough samples
    maybe_recompute_threshold(&state, &entries);

    // Recompute document boost scores
    recompute_document_boosts(&state, &entries);

    // Correlate feedback with inference log
    let feedback_score = if req.thumbs_up { 1i8 } else { -1i8 };
    state
        .km_store
        .update_inference_log_feedback(&req.response_id, feedback_score);

    Ok(Json(FeedbackResponse { ok: true }))
}

// ── Feedback Stats ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct FeedbackStats {
    pub total: usize,
    pub positive: usize,
    pub negative: usize,
    pub positive_rate: f32,
    pub current_threshold: f32,
    pub adaptive_threshold: Option<f32>,
    pub adaptive_enabled: bool,
    pub min_samples: u32,
}

/// GET /api/km/settings/feedback/stats — admin view of feedback stats.
pub async fn get_feedback_stats(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
) -> Json<FeedbackStats> {
    let entries = load_entries(&state);

    let total = entries.len();
    let positive = entries.iter().filter(|e| e.thumbs_up).count();
    let negative = total - positive;
    let positive_rate = if total > 0 {
        positive as f32 / total as f32
    } else {
        0.0
    };

    let config = &state.config.chat_pipeline;
    let adaptive =
        if config.adaptive_threshold_enabled && total >= config.adaptive_min_samples as usize {
            Some(compute_adaptive_threshold(&entries))
        } else {
            None
        };

    Json(FeedbackStats {
        total,
        positive,
        negative,
        positive_rate,
        current_threshold: config.quality_guard_threshold,
        adaptive_threshold: adaptive,
        adaptive_enabled: config.adaptive_threshold_enabled,
        min_samples: config.adaptive_min_samples,
    })
}

// ── Feedback Entries List (for dashboard) ───────────────────────────

#[derive(Deserialize)]
pub struct FeedbackListQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    /// Filter: "positive", "negative", or "all" (default)
    #[serde(default = "default_filter")]
    pub filter: String,
    /// Filter by workspace_id
    pub workspace_id: Option<String>,
}

fn default_limit() -> usize {
    50
}
fn default_filter() -> String {
    "all".to_string()
}

#[derive(Serialize)]
pub struct FeedbackListResponse {
    pub entries: Vec<FeedbackEntry>,
    pub total: usize,
    pub total_filtered: usize,
}

/// GET /api/km/settings/feedback/entries — paginated feedback list.
pub async fn list_feedback_entries(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<FeedbackListQuery>,
) -> Json<FeedbackListResponse> {
    let all = load_entries(&state);
    let total = all.len();

    // Filter
    let filtered: Vec<&FeedbackEntry> = all
        .iter()
        .filter(|e| match q.filter.as_str() {
            "positive" => e.thumbs_up,
            "negative" => !e.thumbs_up,
            _ => true,
        })
        .filter(|e| {
            if let Some(ref ws) = q.workspace_id {
                e.workspace_id.as_deref() == Some(ws.as_str())
            } else {
                true
            }
        })
        .collect();

    let total_filtered = filtered.len();

    // Reverse to show newest first, then paginate
    let entries: Vec<FeedbackEntry> = filtered
        .into_iter()
        .rev()
        .skip(q.offset)
        .take(q.limit)
        .cloned()
        .collect();

    Json(FeedbackListResponse {
        entries,
        total,
        total_filtered,
    })
}

// ── Document Boost Scores ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentBoost {
    pub doc_id: String,
    pub boost: f32,
    pub positive_count: usize,
    pub negative_count: usize,
    pub total_count: usize,
}

/// GET /api/km/settings/feedback/document-boosts — per-document boost scores.
pub async fn get_document_boosts(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
) -> Json<Vec<DocumentBoost>> {
    let boosts: Vec<DocumentBoost> = state
        .km_store
        .get_setting("feedback:document_boosts")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    Json(boosts)
}

/// Recompute document boost scores from all feedback and persist.
fn recompute_document_boosts(state: &AppState, entries: &[FeedbackEntry]) {
    let mut doc_stats: HashMap<String, (usize, usize)> = HashMap::new(); // (positive, negative)

    for entry in entries {
        if entry.doc_ids.is_empty() {
            continue;
        }
        for doc_id in &entry.doc_ids {
            let stat = doc_stats.entry(doc_id.clone()).or_default();
            if entry.thumbs_up {
                stat.0 += 1;
            } else {
                stat.1 += 1;
            }
        }
    }

    let boosts: Vec<DocumentBoost> = doc_stats
        .into_iter()
        .map(|(doc_id, (pos, neg))| {
            let total = pos + neg;
            // Boost formula: positive_rate mapped to [0.5, 1.5]
            // 100% positive → 1.5 (50% boost)
            // 50% positive → 1.0 (neutral)
            // 0% positive → 0.5 (50% penalty)
            // Minimum 3 samples to deviate from neutral
            let boost = if total >= 3 {
                let rate = pos as f32 / total as f32;
                0.5 + rate // range [0.5, 1.5]
            } else {
                1.0 // neutral until enough data
            };
            DocumentBoost {
                doc_id,
                boost,
                positive_count: pos,
                negative_count: neg,
                total_count: total,
            }
        })
        .collect();

    if let Ok(json) = serde_json::to_string(&boosts) {
        state
            .km_store
            .set_setting("feedback:document_boosts", &json);
    }
}

/// Get the document boost map for use in search scoring.
pub fn get_document_boost_map(state: &AppState) -> HashMap<String, f32> {
    state
        .km_store
        .get_setting("feedback:document_boosts")
        .and_then(|json| serde_json::from_str::<Vec<DocumentBoost>>(&json).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|b| (b.boost - 1.0).abs() > 0.01) // Only include non-neutral boosts
        .map(|b| (b.doc_id, b.boost))
        .collect()
}

// ── Golden Examples ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenExample {
    pub id: String,
    pub query: String,
    pub answer: String,
    pub workspace_id: Option<String>,
    pub created_at: i64,
    pub source_response_id: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateGoldenExampleRequest {
    pub response_id: Option<String>,
    pub query: String,
    pub answer: String,
    pub workspace_id: Option<String>,
}

/// POST /api/km/settings/feedback/golden-examples — mark a Q&A pair as golden.
pub async fn create_golden_example(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    AppJson(req): AppJson<CreateGoldenExampleRequest>,
) -> Json<GoldenExample> {
    let example = GoldenExample {
        id: uuid::Uuid::new_v4().to_string(),
        query: req.query,
        answer: req.answer,
        workspace_id: req.workspace_id,
        created_at: Utc::now().timestamp(),
        source_response_id: req.response_id,
    };

    let key = "feedback:golden_examples";
    let mut examples: Vec<GoldenExample> = state
        .km_store
        .get_setting(key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    examples.push(example.clone());

    // Keep max 100 golden examples
    if examples.len() > 100 {
        examples.drain(..examples.len() - 100);
    }

    if let Ok(json) = serde_json::to_string(&examples) {
        state.km_store.set_setting(key, &json);
    }

    Json(example)
}

/// GET /api/km/settings/feedback/golden-examples — list golden examples.
pub async fn list_golden_examples(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
) -> Json<Vec<GoldenExample>> {
    let examples: Vec<GoldenExample> = state
        .km_store
        .get_setting("feedback:golden_examples")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    Json(examples)
}

#[derive(Deserialize)]
pub struct DeleteGoldenExampleQuery {
    pub id: String,
}

/// DELETE /api/km/settings/feedback/golden-examples — remove a golden example.
pub async fn delete_golden_example(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(q): Query<DeleteGoldenExampleQuery>,
) -> Json<FeedbackResponse> {
    let key = "feedback:golden_examples";
    let mut examples: Vec<GoldenExample> = state
        .km_store
        .get_setting(key)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    examples.retain(|e| e.id != q.id);

    if let Ok(json) = serde_json::to_string(&examples) {
        state.km_store.set_setting(key, &json);
    }

    Json(FeedbackResponse { ok: true })
}

/// Load golden examples for a workspace (used by response generator).
pub fn load_golden_examples_for_workspace(
    state: &AppState,
    workspace_id: Option<&str>,
) -> Vec<GoldenExample> {
    let examples: Vec<GoldenExample> = state
        .km_store
        .get_setting("feedback:golden_examples")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    examples
        .into_iter()
        .filter(|e| {
            // Include workspace-specific examples + global examples
            match (&e.workspace_id, workspace_id) {
                (Some(ws), Some(target)) => ws == target,
                (None, _) => true,
                _ => false,
            }
        })
        .take(5) // Max 5 few-shot examples in prompt
        .collect()
}

// ── Retrieval Parameters ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalParams {
    pub top_k: usize,
    pub rrf_k: f32,
    pub vector_weight: f32,
    pub bm25_weight: f32,
    pub min_score_threshold: f32,
    /// Whether auto-tuning is active.
    pub auto_tuned: bool,
    /// Feedback samples used for last auto-tune.
    pub samples_used: usize,
    /// Suggested values from auto-tuning (before applying).
    #[serde(default)]
    pub suggested: Option<SuggestedParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedParams {
    pub top_k: usize,
    pub vector_weight: f32,
    pub bm25_weight: f32,
    pub reason: String,
}

impl Default for RetrievalParams {
    fn default() -> Self {
        Self {
            top_k: 5,
            rrf_k: 60.0,
            vector_weight: 1.0,
            bm25_weight: 1.0,
            min_score_threshold: 0.0,
            auto_tuned: false,
            samples_used: 0,
            suggested: None,
        }
    }
}

/// GET /api/km/settings/feedback/retrieval-params — current retrieval tuning.
pub async fn get_retrieval_params(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
) -> Json<RetrievalParams> {
    let mut params = load_retrieval_params(&state);

    // Compute suggestions from feedback
    let entries = load_entries(&state);
    if entries.len() >= 10 {
        params.suggested = compute_suggested_params(&entries);
    }

    Json(params)
}

#[derive(Deserialize)]
pub struct UpdateRetrievalParamsRequest {
    pub top_k: Option<usize>,
    pub vector_weight: Option<f32>,
    pub bm25_weight: Option<f32>,
    pub min_score_threshold: Option<f32>,
    /// Apply the auto-suggested values.
    #[serde(default)]
    pub apply_suggestions: bool,
}

/// PUT /api/km/settings/feedback/retrieval-params — update retrieval tuning.
pub async fn update_retrieval_params(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    AppJson(req): AppJson<UpdateRetrievalParamsRequest>,
) -> Json<RetrievalParams> {
    let mut params = load_retrieval_params(&state);

    if req.apply_suggestions {
        if let Some(suggested) = &params.suggested {
            params.top_k = suggested.top_k;
            params.vector_weight = suggested.vector_weight;
            params.bm25_weight = suggested.bm25_weight;
            params.auto_tuned = true;
            let entries = load_entries(&state);
            params.samples_used = entries.len();
        }
    } else {
        if let Some(k) = req.top_k {
            params.top_k = k.clamp(1, 20);
        }
        if let Some(vw) = req.vector_weight {
            params.vector_weight = vw.clamp(0.0, 5.0);
        }
        if let Some(bw) = req.bm25_weight {
            params.bm25_weight = bw.clamp(0.0, 5.0);
        }
        if let Some(t) = req.min_score_threshold {
            params.min_score_threshold = t.clamp(0.0, 1.0);
        }
        params.auto_tuned = false;
    }

    save_retrieval_params(&state, &params);

    // Recompute suggestions
    let entries = load_entries(&state);
    if entries.len() >= 10 {
        params.suggested = compute_suggested_params(&entries);
    }

    Json(params)
}

pub fn load_retrieval_params(state: &AppState) -> RetrievalParams {
    state
        .km_store
        .get_setting("feedback:retrieval_params")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save_retrieval_params(state: &AppState, params: &RetrievalParams) {
    if let Ok(json) = serde_json::to_string(params) {
        state
            .km_store
            .set_setting("feedback:retrieval_params", &json);
    }
}

/// Compute suggested retrieval parameter adjustments from feedback patterns.
fn compute_suggested_params(entries: &[FeedbackEntry]) -> Option<SuggestedParams> {
    if entries.len() < 10 {
        return None;
    }

    let total = entries.len();
    let positive = entries.iter().filter(|e| e.thumbs_up).count();
    let positive_rate = positive as f32 / total as f32;

    // Analyze patterns in negative feedback
    let negative_with_chunks: Vec<&FeedbackEntry> = entries
        .iter()
        .filter(|e| !e.thumbs_up && !e.chunk_ids.is_empty())
        .collect();

    let negative_with_low_scores = negative_with_chunks
        .iter()
        .filter(|e| e.chunk_scores.iter().all(|&s| s < 0.01))
        .count();

    let mut reason_parts = Vec::new();
    let mut suggested_top_k = 5;
    let mut suggested_vector_weight = 1.0f32;
    let mut suggested_bm25_weight = 1.0f32;

    if positive_rate < 0.5 {
        // Many negative responses — try retrieving more chunks
        suggested_top_k = 8;
        reason_parts.push(format!(
            "Low satisfaction rate ({:.0}%) — increasing top_k to retrieve more context",
            positive_rate * 100.0
        ));
    } else if positive_rate > 0.85 {
        // Very good — can be more selective
        suggested_top_k = 4;
        reason_parts.push(format!(
            "High satisfaction rate ({:.0}%) — can be more selective with fewer chunks",
            positive_rate * 100.0
        ));
    }

    if !negative_with_chunks.is_empty() {
        let low_score_rate = negative_with_low_scores as f32 / negative_with_chunks.len() as f32;
        if low_score_rate > 0.5 {
            // Most negative feedback has low chunk scores — boost vector search
            suggested_vector_weight = 1.5;
            suggested_bm25_weight = 0.8;
            reason_parts.push(format!(
                "{:.0}% of negative feedback had low relevance scores — boosting vector search weight",
                low_score_rate * 100.0
            ));
        }
    }

    if reason_parts.is_empty() {
        return None;
    }

    Some(SuggestedParams {
        top_k: suggested_top_k,
        vector_weight: suggested_vector_weight,
        bm25_weight: suggested_bm25_weight,
        reason: reason_parts.join(". "),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────

fn load_entries(state: &AppState) -> Vec<FeedbackEntry> {
    state
        .km_store
        .get_setting("feedback:log")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Recompute adaptive threshold and update the pipeline's atomic threshold.
fn maybe_recompute_threshold(state: &AppState, entries: &[FeedbackEntry]) {
    let config = &state.config.chat_pipeline;
    if !config.adaptive_threshold_enabled {
        return;
    }
    if entries.len() < config.adaptive_min_samples as usize {
        return;
    }

    let new_threshold = compute_adaptive_threshold(entries);

    // Update the pipeline's threshold atomically
    let p = state.providers();
    if let Some(ref pipeline) = p.chat_pipeline {
        let handle = pipeline.adaptive_threshold_handle();
        handle.store(new_threshold.to_bits(), Ordering::Relaxed);
        info!(
            threshold = new_threshold,
            samples = entries.len(),
            "Adaptive threshold updated"
        );
    }
}

/// Compute an adaptive threshold from feedback data.
fn compute_adaptive_threshold(entries: &[FeedbackEntry]) -> f32 {
    if entries.is_empty() {
        return 0.6;
    }

    let positive_rate =
        entries.iter().filter(|e| e.thumbs_up).count() as f32 / entries.len() as f32;

    let threshold = if positive_rate >= 0.9 {
        0.4
    } else if positive_rate >= 0.7 {
        0.6 - (positive_rate - 0.7) * 1.0
    } else if positive_rate >= 0.5 {
        0.7 - (positive_rate - 0.5) * 0.5
    } else {
        0.8
    };

    threshold.clamp(0.3, 0.9)
}
