use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobId, JobStoreLookup};
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;

pub async fn download(State(state): State<AppState>, Path(id): Path<String>) -> Result<impl IntoResponse, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;

    let lookup = state
        .jobs
        .with_completed(job_id, |completed| build_zip(completed))
        .ok_or(ApiError::NotFound)?;

    let zip_bytes = match lookup {
        JobStoreLookup::Ready(bytes) => bytes.map_err(ApiError::Internal)?,
        JobStoreLookup::NotReady => return Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => return Err(ApiError::Internal(msg)),
    };

    Ok((
        [(header::CONTENT_TYPE, "application/zip"), (header::CONTENT_DISPOSITION, "attachment; filename=\"serrf-results.zip\"")],
        zip_bytes,
    ))
}

fn build_zip(completed: &crate::job::CompletedJob) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::FileOptions::default();

        zip.start_file("normalized-imputed.csv", options).map_err(|e| e.to_string())?;
        serrf_core::export::write_matrix_csv(&mut zip, &completed.output.sample_order, &completed.compound_labels, &completed.output.raw).map_err(|e| e.to_string())?;

        zip.start_file("normalized-serrf.csv", options).map_err(|e| e.to_string())?;
        serrf_core::export::write_matrix_csv(&mut zip, &completed.output.sample_order, &completed.compound_labels, &completed.output.serrf).map_err(|e| e.to_string())?;

        zip.start_file("qc-rsds.csv", options).map_err(|e| e.to_string())?;
        serrf_core::export::write_rsd_csv(
            &mut zip,
            &completed.compound_labels,
            &completed.output.qc_rsd_raw,
            &completed.output.qc_rsd_serrf,
            &completed.output.validate_rsd_raw,
            &completed.output.validate_rsd_serrf,
        )
        .map_err(|e| e.to_string())?;

        let sds_before: Vec<f64> = (0..completed.output.raw.nrows()).map(|i| serrf_core::export::std_dev(&completed.output.raw.row(i).to_vec())).collect();
        let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.raw, &sds_before));
        let sds_after: Vec<f64> = (0..completed.output.serrf.nrows()).map(|i| serrf_core::export::std_dev(&completed.output.serrf.row(i).to_vec())).collect();
        let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.serrf, &sds_after));

        let png_file = tempfile::Builder::new().suffix(".png").tempfile().map_err(|e| e.to_string())?;
        serrf_core::report::render_report(
            png_file.path(),
            &completed.output.qc_rsd_raw,
            &completed.output.qc_rsd_serrf,
            &pca_before,
            &pca_after,
            &completed.sample_type,
        )
        .map_err(|e| e.to_string())?;
        let png_bytes = std::fs::read(png_file.path()).map_err(|e| e.to_string())?;
        zip.start_file("report.png", options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut zip, &png_bytes).map_err(|e| e.to_string())?;

        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}
