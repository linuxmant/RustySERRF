use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[tokio::test(flavor = "current_thread")]
async fn download_does_not_starve_a_concurrent_ticker_task() {
    let compound_count = 500;
    let sample_count = 50;
    let raw = ndarray::Array2::from_shape_fn((compound_count, sample_count), |(i, j)| ((i + j) % 97) as f64 + 1.0);
    let serrf = raw.clone();
    let completed_job = serrf_api::job::CompletedJob {
        compound_labels: (0..compound_count).map(|i| format!("c{i}")).collect(),
        sample_type: vec![Some("qc".to_string()); sample_count],
        batch: vec!["A".to_string(); sample_count],
        output: serrf_core::PipelineOutput {
            raw,
            serrf,
            qc_rsd_raw: vec![0.1; compound_count],
            qc_rsd_serrf: vec![0.01; compound_count],
            validate_rsd_raw: std::collections::HashMap::new(),
            validate_rsd_serrf: std::collections::HashMap::new(),
            sample_order: (0..sample_count).map(|j| format!("s{j}")).collect(),
        },
    };

    let jobs = serrf_api::job::JobStore::new();
    let (job_id, _rx) = jobs.create();
    jobs.complete(job_id, completed_job);
    let state = serrf_api::app::AppState { jobs };

    let counter = Arc::new(AtomicU64::new(0));
    let ticker_counter = counter.clone();
    let ticker_handle = tokio::spawn(async move {
        loop {
            ticker_counter.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
        }
    });

    // Let the ticker run freely first to establish a baseline
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let before = counter.load(Ordering::Relaxed);
    assert!(before > 0, "ticker task should be advancing before download starts");

    // Spawn the download task
    let download_task = tokio::spawn(serrf_api::routes::download::download(
        axum::extract::State(state),
        axum::extract::Path(job_id.to_string()),
    ));

    // Wait 50ms while download is running (deep in PCA/PNG/zip work)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let during = counter.load(Ordering::Relaxed);

    // Let download finish
    let result = download_task.await.unwrap();
    assert!(result.is_ok(), "download should succeed for a valid completed job");

    ticker_handle.abort();

    let increment = during.saturating_sub(before);
    assert!(
        increment > 10,
        "expected the ticker task to keep advancing while download's PCA/PNG/zip work runs; \
         if increment is very small, download is blocking the single-threaded runtime \
         instead of running via spawn_blocking (before={before}, during={during}, increment={increment})"
    );

    println!(
        "\n=== BLOCKING TEST PASSED ===\nTicker increments during 50ms while /download ran:\n  before: {}\n  during: {}\n  increment: {}",
        before, during, increment
    );
}
