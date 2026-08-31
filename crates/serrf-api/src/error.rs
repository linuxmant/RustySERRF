use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub enum ApiError {
    BadRequest(String),
    NotFound,
    NotReady,
    Internal(String),
    JobFailed(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "job not found".to_string()),
            ApiError::NotReady => (StatusCode::TOO_EARLY, "job is still running".to_string()),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::JobFailed(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<serrf_core::error::SerrfError> for ApiError {
    fn from(e: serrf_core::error::SerrfError) -> Self {
        ApiError::BadRequest(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// I5 regression test: a job that failed because of the data (`JobStoreLookup::Failed`,
    /// surfaced via `ApiError::JobFailed`) must map to 422 Unprocessable Entity, distinct from
    /// `ApiError::Internal`'s 500 — same `{"error": "..."}` body shape as every other variant.
    #[tokio::test]
    async fn job_failed_maps_to_422_unprocessable_entity() {
        let response = ApiError::JobFailed("normalize() rejected the data".to_string()).into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "normalize() rejected the data");
    }

    /// `Internal` must remain a distinct, unchanged 500 — this fix is additive, not a replacement.
    #[tokio::test]
    async fn internal_still_maps_to_500() {
        let response = ApiError::Internal("genuine server bug".to_string()).into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
