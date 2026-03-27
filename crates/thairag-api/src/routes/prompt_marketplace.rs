use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use uuid::Uuid;

use thairag_auth::AuthClaims;
use thairag_core::ThaiRagError;

use crate::app_state::AppState;
use crate::error::{ApiError, AppJson};
use crate::store::{PromptRating, PromptTemplate, PromptTemplateFilter};

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub content: String,
    #[serde(default)]
    pub variables: Vec<String>,
    #[serde(default = "default_true")]
    pub is_public: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct UpdateTemplateRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub variables: Option<Vec<String>>,
    pub is_public: Option<bool>,
}

#[derive(Deserialize)]
pub struct RateRequest {
    pub rating: u8,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
    pub search: Option<String>,
    pub is_public: Option<bool>,
    pub author_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn user_id_from_claims(claims: &AuthClaims) -> String {
    claims.sub.clone()
}

fn user_name_from_claims(claims: &AuthClaims) -> String {
    if claims.email.is_empty() {
        claims.sub.clone()
    } else {
        claims.email.clone()
    }
}

/// Returns true if the user is allowed to mutate this template
/// (they are the author, anonymous, or a super_admin).
fn can_mutate(state: &AppState, template: &PromptTemplate, user_id: &str) -> Result<(), ApiError> {
    if user_id == "anonymous" || template.author_id.as_deref() == Some(user_id) {
        return Ok(());
    }
    if let Ok(uid) = user_id.parse::<uuid::Uuid>() {
        let user = state
            .km_store
            .get_user(thairag_core::types::UserId(uid))
            .map_err(|_| ThaiRagError::Authorization("User not found".into()))?;
        if user.is_super_admin || user.role == "super_admin" {
            return Ok(());
        }
    }
    Err(ThaiRagError::Authorization(
        "Only the author or super_admin can modify this template".into(),
    )
    .into())
}

// ── Handlers ────────────────────────────────────────────────────────

/// GET /api/km/prompts/marketplace
pub async fn list_templates(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<PromptTemplate>>, ApiError> {
    let filter = PromptTemplateFilter {
        category: params.category,
        search: params.search,
        is_public: params.is_public,
        author_id: params.author_id,
        limit: params.limit,
        offset: params.offset,
    };
    let templates = state.km_store.list_prompt_templates(&filter);
    Ok(Json(templates))
}

/// POST /api/km/prompts/marketplace
pub async fn create_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    AppJson(req): AppJson<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<PromptTemplate>), ApiError> {
    if req.name.trim().is_empty() {
        return Err(ThaiRagError::Validation("name must not be empty".into()).into());
    }
    if req.content.trim().is_empty() {
        return Err(ThaiRagError::Validation("content must not be empty".into()).into());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let user_id = user_id_from_claims(&claims);
    let user_name = user_name_from_claims(&claims);

    let template = PromptTemplate {
        id: Uuid::new_v4().to_string(),
        name: req.name,
        description: req.description.unwrap_or_default(),
        category: req.category.unwrap_or_else(|| "general".to_string()),
        content: req.content,
        variables: req.variables,
        author_id: if user_id == "anonymous" {
            None
        } else {
            Some(user_id)
        },
        author_name: if user_name == "anonymous" {
            None
        } else {
            Some(user_name)
        },
        version: 1,
        is_public: req.is_public,
        rating_avg: 0.0,
        rating_count: 0,
        created_at: now.clone(),
        updated_at: now,
    };

    let created = state
        .km_store
        .insert_prompt_template(&template)
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// GET /api/km/prompts/marketplace/:id
pub async fn get_template(
    State(state): State<AppState>,
    Extension(_claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<Json<PromptTemplate>, ApiError> {
    let template = state
        .km_store
        .get_prompt_template(&id)
        .map_err(ApiError::from)?;
    Ok(Json(template))
}

/// PUT /api/km/prompts/marketplace/:id
pub async fn update_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
    AppJson(req): AppJson<UpdateTemplateRequest>,
) -> Result<Json<PromptTemplate>, ApiError> {
    let mut template = state
        .km_store
        .get_prompt_template(&id)
        .map_err(ApiError::from)?;

    // Only author or super_admin can update
    let user_id = user_id_from_claims(&claims);
    can_mutate(&state, &template, &user_id)?;

    if let Some(name) = req.name {
        template.name = name;
    }
    if let Some(desc) = req.description {
        template.description = desc;
    }
    if let Some(cat) = req.category {
        template.category = cat;
    }
    if let Some(content) = req.content {
        template.content = content;
    }
    if let Some(vars) = req.variables {
        template.variables = vars;
    }
    if let Some(pub_flag) = req.is_public {
        template.is_public = pub_flag;
    }
    template.version += 1;

    state
        .km_store
        .update_prompt_template(&template)
        .map_err(ApiError::from)?;
    Ok(Json(template))
}

/// DELETE /api/km/prompts/marketplace/:id
pub async fn delete_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let template = state
        .km_store
        .get_prompt_template(&id)
        .map_err(ApiError::from)?;

    let user_id = user_id_from_claims(&claims);
    can_mutate(&state, &template, &user_id)?;

    state
        .km_store
        .delete_prompt_template(&id)
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/km/prompts/marketplace/:id/rate
pub async fn rate_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
    AppJson(req): AppJson<RateRequest>,
) -> Result<Json<PromptTemplate>, ApiError> {
    if req.rating == 0 || req.rating > 5 {
        return Err(ThaiRagError::Validation("rating must be 1-5".into()).into());
    }

    // Verify template exists
    let _ = state
        .km_store
        .get_prompt_template(&id)
        .map_err(ApiError::from)?;

    let user_id = user_id_from_claims(&claims);
    let rating = PromptRating {
        template_id: id.clone(),
        user_id,
        rating: req.rating,
    };
    state
        .km_store
        .rate_prompt_template(&rating)
        .map_err(ApiError::from)?;

    let updated = state
        .km_store
        .get_prompt_template(&id)
        .map_err(ApiError::from)?;
    Ok(Json(updated))
}

/// POST /api/km/prompts/marketplace/:id/fork
pub async fn fork_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<PromptTemplate>), ApiError> {
    let user_id = user_id_from_claims(&claims);
    let user_name = user_name_from_claims(&claims);

    let forked = state
        .km_store
        .fork_prompt_template(&id, &user_id, &user_name)
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(forked)))
}
