pub mod app;
pub mod error;
pub mod job;
pub mod routes;
#[cfg(feature = "bundled-frontend")]
pub mod static_assets;

#[cfg(feature = "bundled-frontend")]
pub fn browser_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

#[cfg(all(test, feature = "bundled-frontend"))]
mod browser_url_tests {
    use super::browser_url;

    #[test]
    fn formats_localhost_with_the_given_port() {
        assert_eq!(browser_url(8080), "http://127.0.0.1:8080");
    }

    #[test]
    fn formats_a_different_port_correctly() {
        assert_eq!(browser_url(3000), "http://127.0.0.1:3000");
    }
}
