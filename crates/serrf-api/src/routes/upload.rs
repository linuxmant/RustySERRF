use crate::app::AppState;
use crate::error::ApiError;
use crate::job::JobEvent;
use axum::extract::{Multipart, State};
use axum::Json;

pub async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let mut file_name: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
        if field.name() == Some("file") {
            file_name = field.file_name().map(|s| s.to_string());
            bytes = Some(field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.to_vec());
        }
    }

    let file_name = file_name.ok_or_else(|| ApiError::BadRequest("missing 'file' field".to_string()))?;
    let bytes = bytes.ok_or_else(|| ApiError::BadRequest("missing 'file' field".to_string()))?;

    let extension = std::path::Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| ApiError::BadRequest("uploaded file has no extension".to_string()))?;
    if extension != "csv" && extension != "xlsx" {
        return Err(ApiError::BadRequest(format!("unsupported file extension: {extension}")));
    }

    let mut temp_file = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    std::io::Write::write_all(&mut temp_file, &bytes).map_err(|e| ApiError::Internal(e.to_string()))?;

    let dataset = serrf_core::parse::read_data(temp_file.path())?;
    let samples = serrf_core::validate::validate(&dataset)?;

    let (job_id, _rx) = state.jobs.create();
    let jobs = state.jobs.clone();
    let compound_labels = dataset.compounds.label.clone();
    let sample_type = samples.sample_type.clone();

    tokio::task::spawn_blocking(move || {
        let progress_jobs = jobs.clone();
        let result = serrf_core::normalize(&dataset, &samples, &serrf_core::SerrfConfig::default(), move |p| {
            progress_jobs.push_progress(job_id, JobEvent::Progress { stage: p.stage, current: p.current, total: p.total });
        });
        match result {
            Ok(output) => jobs.complete(job_id, crate::job::CompletedJob { compound_labels, sample_type, output }),
            Err(e) => jobs.fail(job_id, e.to_string()),
        }
    });

    Ok((axum::http::StatusCode::ACCEPTED, Json(serde_json::json!({ "job_id": job_id.to_string() }))))
}
