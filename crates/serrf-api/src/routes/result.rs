use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobId, JobStoreLookup};
use axum::extract::{Path, State};
use axum::Json;

#[derive(serde::Serialize)]
pub struct PcaJson {
    pc1: Vec<f64>,
    pc2: Vec<f64>,
    sample_type: Vec<Option<String>>,
    batch: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct ResultJson {
    compound_labels: Vec<String>,
    qc_rsd_raw: Vec<f64>,
    qc_rsd_serrf: Vec<f64>,
    validate_rsd_raw: std::collections::HashMap<String, Vec<f64>>,
    validate_rsd_serrf: std::collections::HashMap<String, Vec<f64>>,
    pca_before: PcaJson,
    pca_after: PcaJson,
}

pub async fn result(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<ResultJson>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;

    let lookup = state
        .jobs
        .with_completed(job_id, |completed| {
            // PCA excludes blank/None-sampleType columns entirely (app.R:1085-1086), matching
            // the report.png path in download.rs — otherwise the frontend renders an extra
            // "unknown" series for a group R never shows.
            let (raw_non_blank, pca_sample_type) = serrf_core::export::select_non_blank_columns(&completed.output.raw, &completed.sample_type);
            let pca_batch = serrf_core::export::select_non_blank_items(&completed.batch, &completed.sample_type);
            let sds_before: Vec<f64> = (0..raw_non_blank.nrows())
                .map(|i| serrf_core::export::std_dev(&raw_non_blank.row(i).to_vec()))
                .collect();
            let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&raw_non_blank, &sds_before));

            let (serrf_non_blank, _) = serrf_core::export::select_non_blank_columns(&completed.output.serrf, &completed.sample_type);
            let sds_after: Vec<f64> = (0..serrf_non_blank.nrows())
                .map(|i| serrf_core::export::std_dev(&serrf_non_blank.row(i).to_vec()))
                .collect();
            let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&serrf_non_blank, &sds_after));

            ResultJson {
                compound_labels: completed.compound_labels.clone(),
                qc_rsd_raw: completed.output.qc_rsd_raw.clone(),
                qc_rsd_serrf: completed.output.qc_rsd_serrf.clone(),
                validate_rsd_raw: completed.output.validate_rsd_raw.clone(),
                validate_rsd_serrf: completed.output.validate_rsd_serrf.clone(),
                pca_before: PcaJson {
                    pc1: pca_before.pc1,
                    pc2: pca_before.pc2,
                    sample_type: pca_sample_type.clone(),
                    batch: pca_batch.clone(),
                },
                pca_after: PcaJson {
                    pc1: pca_after.pc1,
                    pc2: pca_after.pc2,
                    sample_type: pca_sample_type,
                    batch: pca_batch,
                },
            }
        })
        .ok_or(ApiError::NotFound)?;

    match lookup {
        JobStoreLookup::Ready(json) => Ok(Json(json)),
        JobStoreLookup::NotReady => Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => Err(ApiError::JobFailed(msg)),
    }
}
