fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to build the tokio runtime");
    runtime.block_on(async_main());
    // spawn_blocking tasks (e.g. a normalization job or a download/result response being
    // built) are not cancellable, so the runtime's own teardown would otherwise block for
    // however long that work takes. Cap it so a deploy's stop-timeout (Docker/systemd) is
    // never blown through by a job that was already going to be abandoned anyway.
    runtime.shutdown_timeout(std::time::Duration::from_secs(10));
}

async fn async_main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "serrf_api=info,tower_http=info".into()))
        .init();

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let host = resolve_host(std::env::var("HOST").ok().as_deref());
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await.unwrap();
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

/// Interface serrf-api listens on. Defaults to loopback-only regardless of deployment mode: the
/// design only intends the frontend's own port to be reachable externally, with Next.js proxying
/// `/api/*` to serrf-api over loopback (see frontend/next.config.js's `API_INTERNAL_URL`
/// default) rather than serrf-api's port being exposed directly. An operator who genuinely needs
/// serrf-api reachable from other hosts (e.g. it and the frontend run in separate containers)
/// can opt in via the `HOST` env var.
fn resolve_host(env_host: Option<&str>) -> String {
    match env_host {
        Some(host) if !host.is_empty() => host.to_string(),
        _ => "127.0.0.1".to_string(),
    }
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
    use super::{resolve_host, wait_for_shutdown};

    #[test]
    fn defaults_to_loopback_only() {
        // Only the frontend's port is meant to be reachable externally — Next.js proxies
        // `/api/*` to serrf-api over loopback (see frontend/next.config.js's
        // `API_INTERNAL_URL` default), so serrf-api itself should not default to listening on
        // every interface.
        assert_eq!(resolve_host(None), "127.0.0.1");
    }

    #[test]
    fn an_explicit_host_env_var_overrides_the_loopback_default() {
        assert_eq!(resolve_host(Some("0.0.0.0")), "0.0.0.0");
    }

    #[test]
    fn an_empty_host_env_var_falls_back_to_the_loopback_default() {
        assert_eq!(resolve_host(Some("")), "127.0.0.1");
    }

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
