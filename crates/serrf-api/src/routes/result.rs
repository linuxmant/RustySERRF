use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobId, JobStoreLookup};
use axum::extract::{Path, State};
use axum::Json;

#[derive(serde::Serialize)]
pub struct PcaJson {
    pc1: Vec<f64>,
    pc2: Vec<f64>,
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
            let sds_before: Vec<f64> = (0..completed.output.raw.nrows())
                .map(|i| serrf_core::export::std_dev(&completed.output.raw.row(i).to_vec()))
                .collect();
            let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.raw, &sds_before));
            let sds_after: Vec<f64> = (0..completed.output.serrf.nrows())
                .map(|i| serrf_core::export::std_dev(&completed.output.serrf.row(i).to_vec()))
                .collect();
            let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.serrf, &sds_after));

            ResultJson {
                compound_labels: completed.compound_labels.clone(),
                qc_rsd_raw: completed.output.qc_rsd_raw.clone(),
                qc_rsd_serrf: completed.output.qc_rsd_serrf.clone(),
                validate_rsd_raw: completed.output.validate_rsd_raw.clone(),
                validate_rsd_serrf: completed.output.validate_rsd_serrf.clone(),
                pca_before: PcaJson { pc1: pca_before.pc1, pc2: pca_before.pc2 },
                pca_after: PcaJson { pc1: pca_after.pc1, pc2: pca_after.pc2 },
            }
        })
        .ok_or(ApiError::NotFound)?;

    match lookup {
        JobStoreLookup::Ready(json) => Ok(Json(json)),
        JobStoreLookup::NotReady => Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => Err(ApiError::Internal(msg)),
    }
}
