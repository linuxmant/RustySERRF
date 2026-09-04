use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobEvent, JobId};
use axum::extract::{Path, State};
use axum::Json;

pub async fn status(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<JobEvent>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;
    let (_history, rx) = state.jobs.subscribe(job_id).ok_or(ApiError::NotFound)?;
    let event = rx.borrow().clone();
    Ok(Json(event))
}
