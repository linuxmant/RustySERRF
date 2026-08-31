#[derive(Clone)]
pub struct AppState {
    pub jobs: crate::job::JobStore,
}

pub fn build_app() -> axum::Router {
    let state = AppState { jobs: crate::job::JobStore::new() };
    axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .route("/api/jobs/:id/result", axum::routing::get(crate::routes::result::result))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
