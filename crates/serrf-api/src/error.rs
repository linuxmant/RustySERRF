use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::http::StatusCode;

pub enum ApiError {
    BadRequest(String),
    NotFound,
    NotReady,
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "job not found".to_string()),
            ApiError::NotReady => (StatusCode::TOO_EARLY, "job is still running".to_string()),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<serrf_core::error::SerrfError> for ApiError {
    fn from(e: serrf_core::error::SerrfError) -> Self {
        ApiError::BadRequest(e.to_string())
    }
}
