#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "serrf_api=info,tower_http=info".into()))
        .init();

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    #[cfg(feature = "bundled-frontend")]
    let host = "127.0.0.1";
    #[cfg(not(feature = "bundled-frontend"))]
    let host = "0.0.0.0";
    let listener = tokio::net::TcpListener::bind((host, port)).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    tracing::info!("serrf-api listening on {local_addr}");

    #[cfg(feature = "bundled-frontend")]
    {
        let url = serrf_api::browser_url(local_addr.port());
        if let Err(e) = open::that(&url) {
            tracing::warn!("could not open a browser automatically ({e}) — open {url} manually");
        }
    }

    axum::serve(listener, serrf_api::app::build_app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    wait_for_shutdown(ctrl_c, terminate).await;
}

async fn wait_for_shutdown(ctrl_c: impl std::future::Future<Output = std::io::Result<()>>, terminate: impl std::future::Future<Output = ()>) {
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received, shutting down gracefully");
}

#[cfg(test)]
mod tests {
    use super::wait_for_shutdown;

    #[tokio::test]
    async fn resolves_as_soon_as_the_terminate_branch_completes() {
        let ctrl_c = std::future::pending::<std::io::Result<()>>();
        let terminate = async {};

        tokio::time::timeout(std::time::Duration::from_secs(1), wait_for_shutdown(ctrl_c, terminate))
            .await
            .expect("wait_for_shutdown should resolve once the terminate branch completes, not hang");
    }

    #[tokio::test]
    async fn resolves_as_soon_as_the_ctrl_c_branch_completes() {
        let ctrl_c = async { Ok(()) };
        let terminate = std::future::pending::<()>();

        tokio::time::timeout(std::time::Duration::from_secs(1), wait_for_shutdown(ctrl_c, terminate))
            .await
            .expect("wait_for_shutdown should resolve once the ctrl_c branch completes, not hang");
    }
}
