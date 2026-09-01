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

async fn upload_fixture(base_url: &str, client: &reqwest::Client) -> String {
    let part = reqwest::multipart::Part::text(valid_csv_fixture()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    body["job_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn status_eventually_reports_completed() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let job_id = upload_fixture(&base_url, &client).await;

    for _ in 0..100 {
        let response = client.get(format!("{base_url}/api/jobs/{job_id}")).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let body: serde_json::Value = response.json().await.unwrap();
        if body["status"] == "completed" {
            return;
        }
        assert!(
            body["status"] == "queued" || body["status"] == "progress",
            "unexpected status while waiting: {body}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job never reported completed within the polling window");
}

#[tokio::test]
async fn status_for_an_unknown_job_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let fake_id = uuid::Uuid::new_v4();

    let response = client.get(format!("{base_url}/api/jobs/{fake_id}")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn status_for_a_malformed_job_id_returns_400() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/api/jobs/not-a-uuid")).send().await.unwrap();

    assert_eq!(response.status(), 400);
}
