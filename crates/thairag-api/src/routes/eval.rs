use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::types::{
    DocId, EvalMetrics, EvalQuery, EvalQuerySet, EvalResult, EvalSetId, QueryEvalResult,
    SearchQuery,
};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::eval;
use crate::store::RegressionRun;

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateQuerySetRequest {
    pub name: String,
    pub queries: Vec<EvalQueryInput>,
}

#[derive(Deserialize)]
pub struct EvalQueryInput {
    pub query: String,
    pub relevant_doc_ids: Vec<Uuid>,
    #[serde(default)]
    pub relevance_scores: Option<Vec<f32>>,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn require_super_admin(claims: &AuthClaims, state: &AppState) -> Result<(), ApiError> {
    if claims.sub == "anonymous" {
        return Ok(());
    }
    let user_id = claims
        .sub
        .parse::<Uuid>()
        .map(thairag_core::types::UserId)
        .map_err(|_| ThaiRagError::Auth("Invalid user ID".into()))?;
    let user = state
        .km_store
        .get_user(user_id)
        .map_err(|_| ThaiRagError::Authorization("User not found".into()))?;
    if user.is_super_admin || user.role == "super_admin" {
        Ok(())
    } else {
        Err(
            ThaiRagError::Authorization("Only super admins can manage evaluation sets".into())
                .into(),
        )
    }
}

fn eval_set_key(id: &EvalSetId) -> String {
    format!("_eval_set.{}", id.0)
}

fn eval_index_key() -> &'static str {
    "_eval_set_index"
}

fn eval_result_key(query_set_id: &EvalSetId, timestamp: &str) -> String {
    format!("_eval_result.{}.{}", query_set_id.0, timestamp)
}

fn eval_result_index_key(query_set_id: &EvalSetId) -> String {
    format!("_eval_result_index.{}", query_set_id.0)
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /api/km/eval/query-sets
pub async fn create_query_set(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<CreateQuerySetRequest>,
) -> Result<(StatusCode, Json<EvalQuerySet>), ApiError> {
    require_super_admin(&claims, &state)?;

    if req.name.trim().is_empty() {
        return Err(ThaiRagError::Validation("name must not be empty".into()).into());
    }
    if req.queries.is_empty() {
        return Err(ThaiRagError::Validation("queries must not be empty".into()).into());
    }

    let id = EvalSetId::new();
    let queries: Vec<EvalQuery> = req
        .queries
        .into_iter()
        .map(|q| EvalQuery {
            query: q.query,
            relevant_doc_ids: q.relevant_doc_ids.into_iter().map(DocId).collect(),
            relevance_scores: q.relevance_scores,
        })
        .collect();

    let query_set = EvalQuerySet {
        id,
        name: req.name,
        queries,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string(&query_set)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state.km_store.set_setting(&eval_set_key(&id), &json);

    // Update index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting(eval_index_key())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.push(id.0.to_string());
    state
        .km_store
        .set_setting(eval_index_key(), &serde_json::to_string(&ids).unwrap());

    Ok((StatusCode::CREATED, Json(query_set)))
}

/// GET /api/km/eval/query-sets
pub async fn list_query_sets(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<EvalQuerySet>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let index_str = state
        .km_store
        .get_setting(eval_index_key())
        .unwrap_or_default();
    let ids: Vec<String> = if index_str.is_empty() {
        vec![]
    } else {
        serde_json::from_str(&index_str).unwrap_or_default()
    };

    let mut sets = Vec::new();
    for id_str in &ids {
        if let Ok(uuid) = Uuid::parse_str(id_str) {
            let key = eval_set_key(&EvalSetId(uuid));
            if let Some(json) = state.km_store.get_setting(&key)
                && let Ok(qs) = serde_json::from_str::<EvalQuerySet>(&json)
            {
                sets.push(qs);
            }
        }
    }

    Ok(Json(sets))
}

/// GET /api/km/eval/query-sets/:id
pub async fn get_query_set(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<EvalQuerySet>, ApiError> {
    require_super_admin(&claims, &state)?;

    let key = eval_set_key(&EvalSetId(id));
    let json = state
        .km_store
        .get_setting(&key)
        .ok_or_else(|| ThaiRagError::NotFound("Query set not found".into()))?;
    let qs: EvalQuerySet =
        serde_json::from_str(&json).map_err(|e| ThaiRagError::Internal(e.to_string()))?;

    Ok(Json(qs))
}

/// DELETE /api/km/eval/query-sets/:id
pub async fn delete_query_set(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;

    let set_id = EvalSetId(id);
    let key = eval_set_key(&set_id);

    // Check it exists
    if state.km_store.get_setting(&key).is_none() {
        return Err(ThaiRagError::NotFound("Query set not found".into()).into());
    }

    state.km_store.delete_setting(&key);

    // Remove from index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting(eval_index_key())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.retain(|s| s != &id.to_string());
    state
        .km_store
        .set_setting(eval_index_key(), &serde_json::to_string(&ids).unwrap());

    // Also clean up results for this set
    let result_idx_key = eval_result_index_key(&set_id);
    let result_timestamps: Vec<String> = state
        .km_store
        .get_setting(&result_idx_key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for ts in &result_timestamps {
        state.km_store.delete_setting(&eval_result_key(&set_id, ts));
    }
    state.km_store.delete_setting(&result_idx_key);

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/km/eval/query-sets/:id/run
///
/// Run evaluation: for each query, execute the search pipeline,
/// compute IR metrics, store and return the result.
pub async fn run_evaluation(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<EvalResult>, ApiError> {
    require_super_admin(&claims, &state)?;

    let set_id = EvalSetId(id);
    let key = eval_set_key(&set_id);
    let json = state
        .km_store
        .get_setting(&key)
        .ok_or_else(|| ThaiRagError::NotFound("Query set not found".into()))?;
    let query_set: EvalQuerySet =
        serde_json::from_str(&json).map_err(|e| ThaiRagError::Internal(e.to_string()))?;

    let p = state.providers();
    let mut per_query_results = Vec::new();

    for eq in &query_set.queries {
        let start = Instant::now();

        // Run the search pipeline (no workspace filter — unrestricted)
        let search_query = SearchQuery {
            text: eq.query.clone(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };

        let search_results = p
            .search_engine
            .search(&search_query)
            .await
            .map_err(ApiError::from)?;

        let latency_ms = start.elapsed().as_millis() as u64;

        // Extract retrieved doc_ids (deduplicate, preserve order)
        let mut retrieved_doc_ids: Vec<DocId> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sr in &search_results {
            if seen.insert(sr.chunk.doc_id) {
                retrieved_doc_ids.push(sr.chunk.doc_id);
            }
        }

        let ndcg_at_5 = eval::compute_ndcg(
            &retrieved_doc_ids,
            &eq.relevant_doc_ids,
            eq.relevance_scores.as_deref(),
            5,
        );
        let ndcg_at_10 = eval::compute_ndcg(
            &retrieved_doc_ids,
            &eq.relevant_doc_ids,
            eq.relevance_scores.as_deref(),
            10,
        );
        let mrr = eval::compute_mrr(&retrieved_doc_ids, &eq.relevant_doc_ids);
        let precision = eval::compute_precision_at_k(&retrieved_doc_ids, &eq.relevant_doc_ids, 5);
        let recall = eval::compute_recall_at_k(&retrieved_doc_ids, &eq.relevant_doc_ids, 10);

        per_query_results.push(QueryEvalResult {
            query: eq.query.clone(),
            ndcg_at_5,
            ndcg_at_10,
            mrr,
            precision,
            recall,
            latency_ms,
            retrieved_doc_ids,
        });
    }

    // Aggregate metrics
    let n = per_query_results.len() as f64;
    let metrics = if n > 0.0 {
        EvalMetrics {
            ndcg_at_5: per_query_results.iter().map(|r| r.ndcg_at_5).sum::<f64>() / n,
            ndcg_at_10: per_query_results.iter().map(|r| r.ndcg_at_10).sum::<f64>() / n,
            mrr: per_query_results.iter().map(|r| r.mrr).sum::<f64>() / n,
            precision_at_5: per_query_results.iter().map(|r| r.precision).sum::<f64>() / n,
            precision_at_10: {
                // Recalculate P@10 for aggregate
                let sum: f64 = query_set
                    .queries
                    .iter()
                    .zip(per_query_results.iter())
                    .map(|(eq, pqr)| {
                        eval::compute_precision_at_k(
                            &pqr.retrieved_doc_ids,
                            &eq.relevant_doc_ids,
                            10,
                        )
                    })
                    .sum();
                sum / n
            },
            recall_at_10: per_query_results.iter().map(|r| r.recall).sum::<f64>() / n,
            mean_latency_ms: per_query_results
                .iter()
                .map(|r| r.latency_ms as f64)
                .sum::<f64>()
                / n,
        }
    } else {
        EvalMetrics {
            ndcg_at_5: 0.0,
            ndcg_at_10: 0.0,
            mrr: 0.0,
            precision_at_5: 0.0,
            precision_at_10: 0.0,
            recall_at_10: 0.0,
            mean_latency_ms: 0.0,
        }
    };

    let run_at = chrono::Utc::now().to_rfc3339();
    let eval_result = EvalResult {
        query_set_id: set_id,
        run_at: run_at.clone(),
        metrics,
        per_query: per_query_results,
    };

    // Store result
    let result_json = serde_json::to_string(&eval_result)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state
        .km_store
        .set_setting(&eval_result_key(&set_id, &run_at), &result_json);

    // Update result index
    let result_idx_key = eval_result_index_key(&set_id);
    let mut timestamps: Vec<String> = state
        .km_store
        .get_setting(&result_idx_key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    timestamps.push(run_at);
    state.km_store.set_setting(
        &result_idx_key,
        &serde_json::to_string(&timestamps).unwrap(),
    );

    Ok(Json(eval_result))
}

/// GET /api/km/eval/query-sets/:id/results
///
/// List past evaluation results for a query set (most recent first).
pub async fn list_results(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<EvalResult>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let set_id = EvalSetId(id);

    // Check set exists
    if state.km_store.get_setting(&eval_set_key(&set_id)).is_none() {
        return Err(ThaiRagError::NotFound("Query set not found".into()).into());
    }

    let result_idx_key = eval_result_index_key(&set_id);
    let timestamps: Vec<String> = state
        .km_store
        .get_setting(&result_idx_key)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut results = Vec::new();
    for ts in timestamps.iter().rev() {
        if let Some(json) = state.km_store.get_setting(&eval_result_key(&set_id, ts))
            && let Ok(r) = serde_json::from_str::<EvalResult>(&json)
        {
            results.push(r);
        }
    }

    Ok(Json(results))
}

/// POST /api/km/eval/query-sets/import
///
/// Import a query set from CSV format. Expects JSON body with name and csv_data.
/// CSV columns: query, doc_id (one row per query-doc pair, duplicate queries are merged).
#[derive(Deserialize)]
pub struct ImportCsvRequest {
    pub name: String,
    pub csv_data: String,
}

// ── Regression Check ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RegressionCheckRequest {
    pub query_set_id: String,
    pub baseline_score: f64,
}

#[derive(Serialize)]
pub struct RegressionCheckResponse {
    pub run: RegressionRun,
    pub message: String,
}

#[derive(Deserialize)]
pub struct RegressionHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

/// POST /api/km/eval/regression-check
///
/// Run the named query set and compare its score against the provided baseline.
pub async fn run_regression_check(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<RegressionCheckRequest>,
) -> Result<Json<RegressionCheckResponse>, ApiError> {
    require_super_admin(&claims, &state)?;

    // Parse query_set_id
    let set_uuid = Uuid::parse_str(&req.query_set_id)
        .map_err(|_| ThaiRagError::Validation("Invalid query_set_id".into()))?;
    let set_id = EvalSetId(set_uuid);

    let key = eval_set_key(&set_id);
    let json = state
        .km_store
        .get_setting(&key)
        .ok_or_else(|| ThaiRagError::NotFound("Query set not found".into()))?;
    let query_set: EvalQuerySet =
        serde_json::from_str(&json).map_err(|e| ThaiRagError::Internal(e.to_string()))?;

    let p = state.providers();
    let mut per_query_results = Vec::new();

    for eq in &query_set.queries {
        let start = Instant::now();
        let search_query = SearchQuery {
            text: eq.query.clone(),
            top_k: 10,
            workspace_ids: vec![],
            unrestricted: true,
        };
        let search_results = p
            .search_engine
            .search(&search_query)
            .await
            .map_err(ApiError::from)?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let mut retrieved_doc_ids: Vec<DocId> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sr in &search_results {
            if seen.insert(sr.chunk.doc_id) {
                retrieved_doc_ids.push(sr.chunk.doc_id);
            }
        }

        let ndcg_at_10 = eval::compute_ndcg(
            &retrieved_doc_ids,
            &eq.relevant_doc_ids,
            eq.relevance_scores.as_deref(),
            10,
        );
        let mrr = eval::compute_mrr(&retrieved_doc_ids, &eq.relevant_doc_ids);

        per_query_results.push(QueryEvalResult {
            query: eq.query.clone(),
            ndcg_at_5: 0.0,
            ndcg_at_10,
            mrr,
            precision: 0.0,
            recall: 0.0,
            latency_ms,
            retrieved_doc_ids,
        });
    }

    let n = per_query_results.len() as f64;
    let current_score = if n > 0.0 {
        (per_query_results.iter().map(|r| r.ndcg_at_10).sum::<f64>()
            + per_query_results.iter().map(|r| r.mrr).sum::<f64>())
            / (2.0 * n)
    } else {
        0.0
    };

    let degradation = req.baseline_score - current_score;
    let threshold = state.config.search_quality.regression_threshold;
    let passed = degradation <= threshold;

    let details = serde_json::json!({
        "query_count": per_query_results.len(),
        "threshold": threshold,
        "per_query": per_query_results.iter().map(|r| serde_json::json!({
            "query": r.query,
            "ndcg_at_10": r.ndcg_at_10,
            "mrr": r.mrr,
        })).collect::<Vec<_>>(),
    });

    let run = RegressionRun {
        id: Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        query_set_id: req.query_set_id.clone(),
        baseline_score: req.baseline_score,
        current_score,
        degradation,
        passed,
        details: serde_json::to_string(&details).unwrap_or_else(|_| "{}".to_string()),
    };

    state.km_store.insert_regression_run(&run);

    let message = if passed {
        format!(
            "PASSED: score {:.4} is within threshold ({:.4}) of baseline {:.4}",
            current_score, threshold, req.baseline_score
        )
    } else {
        format!(
            "FAILED: score {:.4} degraded {:.4} from baseline {:.4} (threshold {:.4})",
            current_score, degradation, req.baseline_score, threshold
        )
    };

    Ok(Json(RegressionCheckResponse { run, message }))
}

/// GET /api/km/eval/regression-history
pub async fn list_regression_history(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<RegressionHistoryQuery>,
) -> Result<Json<Vec<RegressionRun>>, ApiError> {
    require_super_admin(&claims, &state)?;
    let runs = state.km_store.list_regression_runs(params.limit);
    Ok(Json(runs))
}

// ── Import Query Set ────────────────────────────────────────────────

pub async fn import_query_set(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<ImportCsvRequest>,
) -> Result<(StatusCode, Json<EvalQuerySet>), ApiError> {
    require_super_admin(&claims, &state)?;

    if req.name.trim().is_empty() {
        return Err(ThaiRagError::Validation("name must not be empty".into()).into());
    }
    if req.csv_data.trim().is_empty() {
        return Err(ThaiRagError::Validation("csv_data must not be empty".into()).into());
    }

    // Parse CSV: query,doc_id
    let mut query_map: std::collections::HashMap<String, Vec<DocId>> =
        std::collections::HashMap::new();

    for (line_num, line) in req.csv_data.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip header
        if line_num == 0
            && (line.to_lowercase().starts_with("query") || line.to_lowercase().starts_with("#"))
        {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ',').collect();
        if parts.len() < 2 {
            return Err(ThaiRagError::Validation(format!(
                "Line {}: expected query,doc_id format",
                line_num + 1
            ))
            .into());
        }

        let query = parts[0].trim().trim_matches('"').to_string();
        let doc_id_str = parts[1].trim().trim_matches('"');
        let doc_id = Uuid::parse_str(doc_id_str).map_err(|_| {
            ThaiRagError::Validation(format!(
                "Line {}: invalid doc_id UUID: {}",
                line_num + 1,
                doc_id_str
            ))
        })?;

        query_map.entry(query).or_default().push(DocId(doc_id));
    }

    if query_map.is_empty() {
        return Err(
            ThaiRagError::Validation("No valid query-doc pairs found in CSV".into()).into(),
        );
    }

    let queries: Vec<EvalQuery> = query_map
        .into_iter()
        .map(|(query, relevant_doc_ids)| EvalQuery {
            query,
            relevant_doc_ids,
            relevance_scores: None,
        })
        .collect();

    let id = EvalSetId::new();
    let query_set = EvalQuerySet {
        id,
        name: req.name,
        queries,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string(&query_set)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state.km_store.set_setting(&eval_set_key(&id), &json);

    // Update index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting(eval_index_key())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.push(id.0.to_string());
    state
        .km_store
        .set_setting(eval_index_key(), &serde_json::to_string(&ids).unwrap());

    Ok((StatusCode::CREATED, Json(query_set)))
}
