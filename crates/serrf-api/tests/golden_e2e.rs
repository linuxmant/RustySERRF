async fn spawn_app() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = serrf_api::app::build_app();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn the_full_job_lifecycle_completes_for_the_real_bundled_dataset() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let bytes = std::fs::read("../../golden/example-dataset.xlsx").unwrap();
    let part = reqwest::multipart::Part::bytes(bytes).file_name("example-dataset.xlsx").mime_str("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let upload_response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    assert_eq!(upload_response.status(), 202);
    let body: serde_json::Value = upload_response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    // SSE stream should eventually reach a terminal event; the real dataset takes minutes.
    let events_response = client.get(format!("{base_url}/api/jobs/{job_id}/events")).timeout(std::time::Duration::from_secs(600)).send().await.unwrap();
    assert_eq!(events_response.status(), 200);
    let events_body = events_response.text().await.unwrap();
    assert!(events_body.contains("event: completed"), "expected the real dataset job to complete: {events_body}");

    let result_response = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
    assert_eq!(result_response.status(), 200);
    let result: serde_json::Value = result_response.json().await.unwrap();
    assert_eq!(result["compound_labels"].as_array().unwrap().len(), 268);

    let download_response = client.get(format!("{base_url}/api/jobs/{job_id}/download")).send().await.unwrap();
    assert_eq!(download_response.status(), 200);
    let zip_bytes = download_response.bytes().await.unwrap();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    assert_eq!(archive.len(), 4);
}
