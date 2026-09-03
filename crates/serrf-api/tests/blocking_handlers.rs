async fn spawn_app() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = serrf_api::app::build_app();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn valid_csv_fixture() -> String {
    let mut header = vec!["No".to_string(), "label".to_string()];
    let mut batch_row = vec!["".to_string(), "batch".to_string()];
    let mut type_row = vec!["".to_string(), "sampleType".to_string()];
    let mut time_row = vec!["".to_string(), "time".to_string()];
    for j in 0..20 {
        let is_qc = j < 12;
        let batch = if j % 4 < 2 { "A" } else { "B" };
        header.push(format!("s{j}"));
        batch_row.push(batch.to_string());
        type_row.push(if is_qc { "qc" } else { "sample" }.to_string());
        time_row.push(j.to_string());
    }
    let mut lines = vec![batch_row.join(","), type_row.join(","), time_row.join(","), header.join(",")];
    for i in 0..3 {
        let mut row = vec![(i + 1).to_string(), format!("Compound{i}")];
        for j in 0..20 {
            row.push((100.0 + i as f64 + j as f64 % 3.0).to_string());
        }
        lines.push(row.join(","));
    }
    lines.join("\n")
}

async fn upload_and_wait_for_completion(base_url: &str, client: &reqwest::Client) -> String {
    let part = reqwest::multipart::Part::text(valid_csv_fixture())
        .file_name("dataset.csv")
        .mime_str("text/csv")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    for _ in 0..200 {
        let status = client.get(format!("{base_url}/api/jobs/{job_id}")).send().await.unwrap();
        let body: serde_json::Value = status.json().await.unwrap();
        if body["status"] == "completed" || body["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    job_id
}

// `current_thread` is essential: with only one OS thread available for the whole runtime,
// a `/download` handler that runs its PCA + PNG-render + zip-build work synchronously
// (instead of via `spawn_blocking`, which hands it to tokio's separate blocking thread
// pool) would monopolize that single thread and starve every other request — including
// this test's concurrent `/health` request — until it finished.
#[tokio::test(flavor = "current_thread")]
async fn download_does_not_starve_a_concurrent_health_check() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let job_id = upload_and_wait_for_completion(&base_url, &client).await;

    let download_base_url = base_url.clone();
    let download_client = client.clone();
    let download_job_id = job_id.clone();
    tokio::spawn(async move {
        let _ = download_client
            .get(format!("{download_base_url}/api/jobs/{download_job_id}/download"))
            .send()
            .await;
    });

    // Give the spawned download request a moment to actually enter its handler.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let health = tokio::time::timeout(std::time::Duration::from_millis(500), client.get(format!("{base_url}/health")).send()).await;

    assert!(
        health.is_ok(),
        "a concurrent /health request should complete quickly even while /download is building its zip; \
         a timeout here means build_zip is running synchronously on the single-threaded runtime instead \
         of via spawn_blocking"
    );
    assert_eq!(health.unwrap().unwrap().status(), 200);
}
