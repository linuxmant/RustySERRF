use std::io;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

async fn spawn_app() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = serrf_api::app::build_app();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[derive(Clone, Default)]
struct CapturingWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturingWriter {
    type Writer = CapturingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

// `current_thread` is required here: `tracing::subscriber::set_default` installs a
// thread-local default, and the server we spawn via `tokio::spawn` must run its request
// handling on that SAME OS thread for the capture to see it. On the default multi-thread
// test runtime, the spawned task could land on a different worker thread and this test
// would flake (capture empty, even though the layer is correctly wired).
#[tokio::test(flavor = "current_thread")]
async fn http_requests_emit_tracing_output_via_tower_http_trace_layer() {
    let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let writer = CapturingWriter(buffer.clone());
    // `with_max_level(TRACE)` makes this test independent of whatever default level
    // main.rs's own subscriber uses in production — we only need to prove the
    // TraceLayer is wired into the router and emits *something* per request.
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let response = client.get(format!("{base_url}/health")).send().await.unwrap();
    assert_eq!(response.status(), 200);

    let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("/health"),
        "expected tower_http's TraceLayer to log the request path, got: {output}"
    );
}
