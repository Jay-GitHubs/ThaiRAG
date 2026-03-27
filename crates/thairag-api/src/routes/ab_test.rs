use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;
use thairag_core::permission::AccessScope;
use thairag_core::types::{
    AbMetrics, AbQueryResult, AbQueryVariantResult, AbTest, AbTestId, AbTestResults, AbTestStatus,
    AbVariant, ChatMessage, SearchOverrides, SearchQuery,
};

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateAbTestRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub variant_a: AbVariantInput,
    pub variant_b: AbVariantInput,
}

#[derive(Deserialize)]
pub struct AbVariantInput {
    pub name: String,
    #[serde(default)]
    pub search_config: Option<SearchOverrides>,
    #[serde(default)]
    pub llm_model: Option<String>,
    #[serde(default)]
    pub prompt_template: Option<String>,
}

#[derive(Deserialize)]
pub struct RunAbTestRequest {
    pub queries: Vec<String>,
}

#[derive(Deserialize)]
pub struct CompareRequest {
    pub query: String,
}

// ── KV Key Helpers ──────────────────────────────────────────────────

fn ab_test_key(id: &AbTestId) -> String {
    format!("_ab_test.{}", id.0)
}

fn ab_test_index_key() -> &'static str {
    "_ab_test_index"
}

// ── Auth Helper ─────────────────────────────────────────────────────

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
        Err(ThaiRagError::Authorization("Only super admins can manage A/B tests".into()).into())
    }
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /api/admin/ab-tests
pub async fn create_ab_test(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<CreateAbTestRequest>,
) -> Result<(StatusCode, Json<AbTest>), ApiError> {
    require_super_admin(&claims, &state)?;

    if req.name.trim().is_empty() {
        return Err(ThaiRagError::Validation("name must not be empty".into()).into());
    }

    let id = AbTestId::new();
    let test = AbTest {
        id,
        name: req.name,
        description: req.description,
        variant_a: AbVariant {
            name: req.variant_a.name,
            search_config: req.variant_a.search_config,
            llm_model: req.variant_a.llm_model,
            prompt_template: req.variant_a.prompt_template,
        },
        variant_b: AbVariant {
            name: req.variant_b.name,
            search_config: req.variant_b.search_config,
            llm_model: req.variant_b.llm_model,
            prompt_template: req.variant_b.prompt_template,
        },
        status: AbTestStatus::Draft,
        created_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
        results: None,
    };

    let json = serde_json::to_string(&test)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state.km_store.set_setting(&ab_test_key(&id), &json);

    // Update index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting(ab_test_index_key())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.push(id.0.to_string());
    state
        .km_store
        .set_setting(ab_test_index_key(), &serde_json::to_string(&ids).unwrap());

    Ok((StatusCode::CREATED, Json(test)))
}

/// GET /api/admin/ab-tests
pub async fn list_ab_tests(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<AbTest>>, ApiError> {
    require_super_admin(&claims, &state)?;

    let ids: Vec<String> = state
        .km_store
        .get_setting(ab_test_index_key())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut tests = Vec::new();
    for id_str in &ids {
        let key = format!("_ab_test.{id_str}");
        if let Some(json) = state.km_store.get_setting(&key)
            && let Ok(test) = serde_json::from_str::<AbTest>(&json)
        {
            tests.push(test);
        }
    }

    // Newest first
    tests.reverse();

    Ok(Json(tests))
}

/// GET /api/admin/ab-tests/:id
pub async fn get_ab_test(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<AbTest>, ApiError> {
    require_super_admin(&claims, &state)?;

    let test_id = AbTestId(id);
    let json = state
        .km_store
        .get_setting(&ab_test_key(&test_id))
        .ok_or_else(|| ThaiRagError::NotFound("A/B test not found".into()))?;
    let test: AbTest =
        serde_json::from_str(&json).map_err(|e| ThaiRagError::Internal(e.to_string()))?;
    Ok(Json(test))
}

/// DELETE /api/admin/ab-tests/:id
pub async fn delete_ab_test(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_super_admin(&claims, &state)?;

    let test_id = AbTestId(id);
    state.km_store.delete_setting(&ab_test_key(&test_id));

    // Update index
    let mut ids: Vec<String> = state
        .km_store
        .get_setting(ab_test_index_key())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    ids.retain(|i| i != &id.to_string());
    state
        .km_store
        .set_setting(ab_test_index_key(), &serde_json::to_string(&ids).unwrap());

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/ab-tests/:id/run — run the A/B test with the given queries
pub async fn run_ab_test(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    AppJson(req): AppJson<RunAbTestRequest>,
) -> Result<Json<AbTest>, ApiError> {
    require_super_admin(&claims, &state)?;

    if req.queries.is_empty() {
        return Err(ThaiRagError::Validation("queries must not be empty".into()).into());
    }

    let test_id = AbTestId(id);
    let json = state
        .km_store
        .get_setting(&ab_test_key(&test_id))
        .ok_or_else(|| ThaiRagError::NotFound("A/B test not found".into()))?;
    let mut test: AbTest =
        serde_json::from_str(&json).map_err(|e| ThaiRagError::Internal(e.to_string()))?;

    // Mark as running
    test.status = AbTestStatus::Running;
    let running_json = serde_json::to_string(&test)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state
        .km_store
        .set_setting(&ab_test_key(&test_id), &running_json);

    // Run queries against both variants
    let mut per_query = Vec::new();
    let mut a_latencies = Vec::new();
    let mut a_relevances = Vec::new();
    let mut a_tokens = Vec::new();
    let mut b_latencies = Vec::new();
    let mut b_relevances = Vec::new();
    let mut b_tokens = Vec::new();

    for query_text in &req.queries {
        let result_a = run_variant_query(&state, &test.variant_a, query_text).await?;
        let result_b = run_variant_query(&state, &test.variant_b, query_text).await?;

        a_latencies.push(result_a.latency_ms as f64);
        a_relevances.push(result_a.relevance_score);
        a_tokens.push(result_a.token_count as f64);

        b_latencies.push(result_b.latency_ms as f64);
        b_relevances.push(result_b.relevance_score);
        b_tokens.push(result_b.token_count as f64);

        per_query.push(AbQueryResult {
            query: query_text.clone(),
            variant_a: result_a,
            variant_b: result_b,
        });
    }

    let n = req.queries.len();
    let a_metrics = AbMetrics {
        avg_latency_ms: a_latencies.iter().sum::<f64>() / n as f64,
        avg_relevance_score: a_relevances.iter().sum::<f64>() / n as f64,
        total_queries: n,
        avg_token_count: a_tokens.iter().sum::<f64>() / n as f64,
    };
    let b_metrics = AbMetrics {
        avg_latency_ms: b_latencies.iter().sum::<f64>() / n as f64,
        avg_relevance_score: b_relevances.iter().sum::<f64>() / n as f64,
        total_queries: n,
        avg_token_count: b_tokens.iter().sum::<f64>() / n as f64,
    };

    // Determine winner: prefer higher relevance, break ties by lower latency
    let winner = if a_metrics.avg_relevance_score > b_metrics.avg_relevance_score + 0.01 {
        Some(test.variant_a.name.clone())
    } else if b_metrics.avg_relevance_score > a_metrics.avg_relevance_score + 0.01 {
        Some(test.variant_b.name.clone())
    } else if a_metrics.avg_latency_ms < b_metrics.avg_latency_ms {
        Some(test.variant_a.name.clone())
    } else if b_metrics.avg_latency_ms < a_metrics.avg_latency_ms {
        Some(test.variant_b.name.clone())
    } else {
        None // tie
    };

    test.status = AbTestStatus::Completed;
    test.completed_at = Some(chrono::Utc::now().to_rfc3339());
    test.results = Some(AbTestResults {
        variant_a_metrics: a_metrics,
        variant_b_metrics: b_metrics,
        winner,
        per_query,
    });

    let final_json = serde_json::to_string(&test)
        .map_err(|e| ApiError(ThaiRagError::Internal(e.to_string())))?;
    state
        .km_store
        .set_setting(&ab_test_key(&test_id), &final_json);

    Ok(Json(test))
}

/// POST /api/admin/ab-tests/:id/compare — side-by-side single query comparison
pub async fn compare_ab_test(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    AppJson(req): AppJson<CompareRequest>,
) -> Result<Json<AbQueryResult>, ApiError> {
    require_super_admin(&claims, &state)?;

    if req.query.trim().is_empty() {
        return Err(ThaiRagError::Validation("query must not be empty".into()).into());
    }

    let test_id = AbTestId(id);
    let json = state
        .km_store
        .get_setting(&ab_test_key(&test_id))
        .ok_or_else(|| ThaiRagError::NotFound("A/B test not found".into()))?;
    let test: AbTest =
        serde_json::from_str(&json).map_err(|e| ThaiRagError::Internal(e.to_string()))?;

    let result_a = run_variant_query(&state, &test.variant_a, &req.query).await?;
    let result_b = run_variant_query(&state, &test.variant_b, &req.query).await?;

    Ok(Json(AbQueryResult {
        query: req.query,
        variant_a: result_a,
        variant_b: result_b,
    }))
}

// ── Variant Execution ───────────────────────────────────────────────

/// Run a single query through one A/B variant configuration.
///
/// The variant can override search parameters (top_k, vector_weight, etc.)
/// and provide a custom prompt template. The query is run through the full
/// search + LLM pipeline using the orchestrator.
async fn run_variant_query(
    state: &AppState,
    variant: &AbVariant,
    query_text: &str,
) -> Result<AbQueryVariantResult, ApiError> {
    let start = Instant::now();
    let p = state.providers();

    // Build search query with optional overrides
    let search_config = &state.config.search;
    let top_k = variant
        .search_config
        .as_ref()
        .and_then(|s| s.top_k)
        .unwrap_or(search_config.top_k);

    let search_query = SearchQuery {
        text: query_text.to_string(),
        top_k,
        workspace_ids: vec![],
        unrestricted: true,
    };

    let search_results = p
        .search_engine
        .search(&search_query)
        .await
        .map_err(ApiError::from)?;

    let chunks_retrieved = search_results.len();
    let avg_score = if search_results.is_empty() {
        0.0
    } else {
        search_results.iter().map(|r| r.score as f64).sum::<f64>() / search_results.len() as f64
    };

    // Build context from search results
    let context: String = search_results
        .iter()
        .take(top_k)
        .enumerate()
        .map(|(i, r)| format!("[{}] {}", i + 1, r.chunk.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Build prompt — use variant's prompt template if provided, otherwise default
    let system_prompt = variant.prompt_template.clone().unwrap_or_else(|| {
        "You are a helpful assistant. Answer the user's question based on the provided context. \
         If the context doesn't contain relevant information, say so."
            .to_string()
    });

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: format!("{system_prompt}\n\nContext:\n{context}"),
        },
        ChatMessage {
            role: "user".to_string(),
            content: query_text.to_string(),
        },
    ];

    // Use the orchestrator to generate the answer (full pipeline)
    let scope = AccessScope::unrestricted();
    let llm_resp = p
        .orchestrator
        .process(&messages, &scope)
        .await
        .map_err(ApiError::from)?;

    let latency_ms = start.elapsed().as_millis() as u64;
    let token_count = llm_resp.usage.prompt_tokens + llm_resp.usage.completion_tokens;

    Ok(AbQueryVariantResult {
        answer: llm_resp.content,
        latency_ms,
        token_count,
        relevance_score: avg_score,
        chunks_retrieved,
    })
}
