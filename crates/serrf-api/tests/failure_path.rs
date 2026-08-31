async fn spawn_app() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = serrf_api::app::build_app();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn csv_with_one_all_missing_compound() -> String {
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
    // Compound0 is normal; Compound1 is entirely missing (blank cells) in every sample.
    let mut normal_row = vec!["1".to_string(), "Compound0".to_string()];
    let mut missing_row = vec!["2".to_string(), "Compound1".to_string()];
    for j in 0..20 {
        normal_row.push((100.0 + j as f64 % 3.0).to_string());
        missing_row.push(String::new());
    }
    lines.push(normal_row.join(","));
    lines.push(missing_row.join(","));
    lines.join("\n")
}

#[tokio::test]
async fn a_job_with_one_unnormalizable_compound_still_completes_successfully() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text(csv_with_one_all_missing_compound()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    for _ in 0..100 {
        let status = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
        if status.status() == 200 {
            let result: serde_json::Value = status.json().await.unwrap();
            assert_eq!(result["compound_labels"].as_array().unwrap().len(), 2);
            // Compound1 (index 1) comes back as NaN, not a job failure.
            assert!(result["qc_rsd_serrf"][1].as_f64().is_none(), "expected NaN (non-numeric in JSON) for the unnormalizable compound");
            assert!(result["qc_rsd_serrf"][0].as_f64().is_some(), "expected Compound0 to normalize normally");
            return;
        }
        assert_ne!(status.status(), 500, "job should not fail outright because of one bad compound");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job never completed within the polling window");
}
