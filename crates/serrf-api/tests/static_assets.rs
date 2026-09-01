#![cfg(feature = "bundled-frontend")]

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
async fn root_serves_the_embedded_index_html() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/")).send().await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "text/html");
    let body = response.text().await.unwrap();
    assert!(body.contains("SERRF (dev placeholder)"));
}

#[tokio::test]
async fn unknown_path_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/does-not-exist")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}
