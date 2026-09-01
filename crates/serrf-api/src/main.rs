#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
    println!("serrf-api listening on {}", listener.local_addr().unwrap());

    #[cfg(feature = "bundled-frontend")]
    {
        let url = serrf_api::browser_url(port);
        if let Err(e) = open::that(&url) {
            eprintln!("Could not open a browser automatically ({e}) — open {url} manually.");
        }
    }

    axum::serve(listener, serrf_api::app::build_app()).await.unwrap();
}
