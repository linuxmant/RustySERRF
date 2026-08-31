#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
    println!("serrf-api listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, serrf_api::app::build_app()).await.unwrap();
}
