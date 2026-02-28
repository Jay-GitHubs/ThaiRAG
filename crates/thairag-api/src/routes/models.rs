use axum::Json;
use serde_json::{Value, json};

pub async fn list_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": "ThaiRAG-1.0",
                "object": "model",
                "created": 1700000000_i64,
                "owned_by": "thairag",
            }
        ]
    }))
}
