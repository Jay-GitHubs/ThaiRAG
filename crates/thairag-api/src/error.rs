use axum::extract::FromRequest;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use thairag_core::ThaiRagError;

pub struct ApiError(pub ThaiRagError);

impl From<ThaiRagError> for ApiError {
    fn from(err: ThaiRagError) -> Self {
        Self(err)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self(ThaiRagError::Validation(rejection.body_text()))
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

        // Log full error details internally (not exposed to client)
        tracing::warn!(
            status = %status,
            error_type = %error_type,
            message = %self.0,
            "API error"
        );

        // LLM02: Sanitize error messages — strip upstream provider details from client response.
        // Only return detailed messages for client-facing error types.
        let client_message = match &self.0 {
            ThaiRagError::Validation(msg) => msg.clone(),
            ThaiRagError::Auth(msg) => msg.clone(),
            ThaiRagError::Authorization(msg) => msg.clone(),
            ThaiRagError::NotFound(msg) => msg.clone(),
            // Internal errors: strip upstream details to prevent information disclosure
            ThaiRagError::LlmProvider(_) => {
                "An error occurred while processing your request with the language model.".into()
            }
            ThaiRagError::Embedding(_) => "An error occurred during embedding processing.".into(),
            ThaiRagError::VectorStore(_) => {
                "An error occurred accessing the knowledge base.".into()
            }
            ThaiRagError::Database(_) => "A database error occurred.".into(),
            ThaiRagError::Config(_) => "A server configuration error occurred.".into(),
            ThaiRagError::Internal(_) => "An internal server error occurred.".into(),
        };

        let body = serde_json::json!({
            "error": {
                "message": client_message,
                "type": error_type,
            }
        });

        (status, axum::Json(body)).into_response()
    }
}

/// A JSON extractor that maps deserialization failures to `ApiError` (400 validation_error)
/// instead of axum's default 422 response.
pub struct AppJson<T>(pub T);

impl<T, S> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(req, state).await?;
        Ok(Self(value))
    }
}
