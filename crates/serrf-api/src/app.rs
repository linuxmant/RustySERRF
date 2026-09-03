#[derive(Clone)]
pub struct AppState {
    pub jobs: crate::job::JobStore,
}

pub fn build_app() -> axum::Router {
    let state = AppState {
        jobs: crate::job::JobStore::new(),
    };

    let sweep_jobs = state.jobs.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
        loop {
            interval.tick().await;
            sweep_jobs.evict_expired(std::time::Duration::from_secs(30 * 60));
        }
    });

    let router = axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id", axum::routing::get(crate::routes::status::status))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .route("/api/jobs/:id/result", axum::routing::get(crate::routes::result::result))
        .route("/api/jobs/:id/download", axum::routing::get(crate::routes::download::download))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    // Note: `Router::layer` only wraps routes/fallback already registered at the time it's
    // called, so the CORS/body-limit layers above do NOT cover a fallback registered afterward.
    // Verified against axum's routing internals — don't assume the static fallback below is
    // covered by them (e.g. don't add a body-consuming fallback here expecting the 10MB limit).
    #[cfg(feature = "bundled-frontend")]
    let router = router.fallback(static_fallback);

    router
}

async fn health() -> &'static str {
    "ok"
}

#[cfg(feature = "bundled-frontend")]
async fn static_fallback(uri: axum::http::Uri) -> axum::response::Response {
    use axum::response::IntoResponse;

    let path = uri.path().trim_start_matches('/');
    match crate::static_assets::lookup(path) {
        Some((bytes, mime)) => ([(axum::http::header::CONTENT_TYPE, mime)], bytes.into_owned()).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
