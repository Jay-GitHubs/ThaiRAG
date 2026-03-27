//! V2 Models endpoint.
//!
//! `GET /v2/models`
//!
//! Returns the same model list as V1 but with additional version metadata.

use axum::Json;
use serde_json::{Value, json};

pub async fn list_models_v2() -> Json<Value> {
    Json(json!({
        "object": "list",
        "api_version": "v2",
        "data": [
            {
                "id": "ThaiRAG-1.0",
                "object": "model",
                "created": 1700000000_i64,
                "owned_by": "thairag",
                "capabilities": {
                    "chat_completions": true,
                    "search": true,
                    "streaming": true,
                },
                "supported_api_versions": ["v1", "v2"],
            }
        ]
    }))
}
