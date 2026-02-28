use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thairag_core::ThaiRagError;

pub struct ApiError(pub ThaiRagError);

impl From<ThaiRagError> for ApiError {
    fn from(err: ThaiRagError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self.0 {
            ThaiRagError::Auth(_) => (StatusCode::UNAUTHORIZED, "authentication_error"),
            ThaiRagError::Authorization(_) => (StatusCode::FORBIDDEN, "authorization_error"),
            ThaiRagError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ThaiRagError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            ThaiRagError::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, "config_error"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = serde_json::json!({
            "error": {
                "message": self.0.to_string(),
                "type": error_type,
            }
        });

        (status, axum::Json(body)).into_response()
    }
}
