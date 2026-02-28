use axum::Json;
use serde_json::{Value, json};

/// Stub: list organizations
pub async fn list_orgs() -> Json<Value> {
    Json(json!({
        "data": [],
        "total": 0,
    }))
}

/// Stub: create organization
pub async fn create_org(Json(_body): Json<Value>) -> Json<Value> {
    todo!("KM org creation not yet implemented")
}
