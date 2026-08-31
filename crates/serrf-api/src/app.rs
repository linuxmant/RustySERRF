pub fn build_app() -> axum::Router {
    axum::Router::new().route("/health", axum::routing::get(health))
}

async fn health() -> &'static str {
    "ok"
}
