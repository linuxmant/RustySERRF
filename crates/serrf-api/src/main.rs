#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    #[cfg(feature = "bundled-frontend")]
    let host = "127.0.0.1";
    #[cfg(not(feature = "bundled-frontend"))]
    let host = "0.0.0.0";
    let listener = tokio::net::TcpListener::bind((host, port)).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    println!("serrf-api listening on {local_addr}");

    #[cfg(feature = "bundled-frontend")]
    {
        let url = serrf_api::browser_url(local_addr.port());
        if let Err(e) = open::that(&url) {
            eprintln!("Could not open a browser automatically ({e}) — open {url} manually.");
        }
    }

    axum::serve(listener, serrf_api::app::build_app()).await.unwrap();
}
