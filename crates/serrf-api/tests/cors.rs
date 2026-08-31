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
async fn cross_origin_requests_get_an_allow_origin_header() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{base_url}/health"))
        .header("Origin", "http://localhost:3000")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert!(
        response.headers().get("access-control-allow-origin").is_some(),
        "expected an Access-Control-Allow-Origin header on a cross-origin request"
    );
}
