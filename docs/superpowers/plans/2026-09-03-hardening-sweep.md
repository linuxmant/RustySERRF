# Hardening Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear 11 previously-deferred hardening/polish items across `serrf-api` and the Next.js frontend: JobStore eviction, tracing, graceful shutdown, SSE keep-alive, moving CPU-bound work off async handlers, a CORS/bind documentation note, and 5 small frontend fixes.

**Architecture:** No new subsystems. Each task is a small, independent change to existing code, following the patterns already established in this repo (integration tests in `crates/serrf-api/tests/*.rs` each define their own `spawn_app()` helper; frontend unit tests use Vitest + Testing Library; `vitest.setup.ts` already stubs `ResizeObserver` and `window.matchMedia`).

**Tech Stack:** Rust (axum 0.7, tokio, tower-http), Next.js 16 / React 19 / MUI 9, Vitest.

**Spec:** none — bounded-path design approved in chat during brainstorming (see Global Constraints below and each task's rationale for the agreed decisions). No separate spec document was written, per the brainstorming skill's bounded-path convention for a well-scoped change to existing code.

## Global Constraints

- Work happens on branch `chore/hardening-sweep`, branched from `master` at the current tip. Never commit directly to `master`.
- Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo clippy -p serrf-api --all-targets --features bundled-frontend -- -D warnings` must all pass (matches `.github/workflows/ci.yml`'s `ci` job exactly).
- Rust integration tests live in `crates/serrf-api/tests/`. Each file defines its own private `spawn_app() -> String` helper (see `crates/serrf-api/tests/cors.rs` and `crates/serrf-api/tests/events.rs`) — do not extract a shared test-helper module; that is not this codebase's convention.
- Frontend: `npm run lint` and `npm run typecheck` must both pass (matches CI's `frontend` job). `frontend/vitest.setup.ts` already stubs `ResizeObserver` and `window.matchMedia` globally — do not re-stub them in individual test files.
- CI runners run roughly 2x slower than local dev machines (established in this project's history) — account for this when a test has any timing sensitivity.
- Follow TDD: write the failing test first, watch it fail for the stated reason, then implement.
- Every git commit in this plan must be on `chore/hardening-sweep`, never `master`.

---

### Task 1: Structured logging via `tracing`

**Files:**
- Modify: `crates/serrf-api/Cargo.toml`
- Modify: `crates/serrf-api/src/main.rs`
- Modify: `crates/serrf-api/src/app.rs`
- Test: `crates/serrf-api/tests/tracing_layer.rs` (create)

**Interfaces:**
- Consumes: `serrf_api::app::build_app() -> axum::Router` (existing, unchanged signature).
- Produces: nothing new is exported. `main.rs` gains a `tracing_subscriber::fmt()` init call; `build_app()`'s router gains a `tower_http::trace::TraceLayer` layer. No other task depends on this one's internals.

- [ ] **Step 1: Write the failing test**

Create `crates/serrf-api/tests/tracing_layer.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-api --test tracing_layer`
Expected: FAIL to compile (`tracing_subscriber` is not yet a dependency of `serrf-api`), or if it compiles once dev-dependencies are guessed, FAIL at runtime with an empty captured buffer (no `TraceLayer` wired into `build_app()` yet).

- [ ] **Step 3: Add dependencies**

In `crates/serrf-api/Cargo.toml`, change the `tower-http` line and add two new dependencies (keep everything else in `[dependencies]` unchanged):

```toml
tower-http = { version = "0.5", features = ["cors", "trace"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Add to `[dev-dependencies]` (keep the existing `reqwest`, `ndarray`, `uuid` lines):

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

(`tracing-subscriber` needs to appear in both `[dependencies]` and `[dev-dependencies]` since `main.rs` uses it in the binary and `tests/tracing_layer.rs` uses it in a test — Cargo requires it listed under both tables even though it resolves to one version.)

- [ ] **Step 4: Wire `TraceLayer` into the router**

In `crates/serrf-api/src/app.rs`, add the trace layer as the outermost layer (added last, so it wraps everything including CORS):

```rust
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);
```

(This replaces the two-layer chain ending in `.with_state(state);` with the three-layer chain above — everything else in `app.rs` is unchanged.)

- [ ] **Step 5: Initialize the subscriber and replace `println!`/`eprintln!` in `main.rs`**

Replace the full contents of `crates/serrf-api/src/main.rs` with:

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "serrf_api=info,tower_http=info".into()),
        )
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

    axum::serve(listener, serrf_api::app::build_app()).await.unwrap();
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p serrf-api --test tracing_layer`
Expected: PASS

- [ ] **Step 7: Run the fast workspace suite (excluding slow golden tests) plus fmt/clippy**

Run: `cargo test --workspace --lib --bins -- --skip golden` then `cargo fmt --check` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo clippy -p serrf-api --all-targets --features bundled-frontend -- -D warnings`
Expected: all pass

- [ ] **Step 8: Commit**

```bash
git add crates/serrf-api/Cargo.toml Cargo.lock crates/serrf-api/src/main.rs crates/serrf-api/src/app.rs crates/serrf-api/tests/tracing_layer.rs
git commit -m "Add structured logging via tracing + tower_http TraceLayer"
```

---

### Task 2: Graceful shutdown

**Files:**
- Modify: `crates/serrf-api/Cargo.toml`
- Modify: `crates/serrf-api/src/main.rs`

**Interfaces:**
- Consumes: `main.rs` as left by Task 1 (tracing already initialized; `tracing::info!` available).
- Produces: nothing exported outside `main.rs`. The `wait_for_shutdown` helper is a private, unit-tested function inside `main.rs`'s own `#[cfg(test)]` module (binaries can contain inline tests; `cargo test --workspace` runs them).

- [ ] **Step 1: Write the failing test**

Add to the bottom of `crates/serrf-api/src/main.rs` (after the `main` function):

```rust
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
```

Note: this references `wait_for_shutdown`, which does not exist yet — that's the point of this step.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-api --bin serrf-api`
Expected: FAIL to compile — `cannot find function 'wait_for_shutdown' in this scope`.

- [ ] **Step 3: Define `wait_for_shutdown` and add the `signal` feature to `tokio`**

Add this function to `crates/serrf-api/src/main.rs`, directly above the `#[cfg(test)] mod tests` block added in Step 1:

```rust
async fn wait_for_shutdown(ctrl_c: impl std::future::Future<Output = std::io::Result<()>>, terminate: impl std::future::Future<Output = ()>) {
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received, shutting down gracefully");
}
```

In `crates/serrf-api/Cargo.toml`, change the `tokio` line:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "signal"] }
```

- [ ] **Step 4: Wire real shutdown signals into `main()`**

In `crates/serrf-api/src/main.rs`, replace this line:

```rust
    axum::serve(listener, serrf_api::app::build_app()).await.unwrap();
```

with:

```rust
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
```

Note the closing `}` that used to end `main()` is now followed by the new `shutdown_signal` function, which itself ends with `}` before the existing `wait_for_shutdown` function from Step 1. Re-read the whole file after this edit to confirm brace balance: `main()` ends right after the `.unwrap();` line's block closes, then `shutdown_signal()` is a new top-level `async fn`, then `wait_for_shutdown` and the `#[cfg(test)]` module from Step 1 follow unchanged.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo build -p serrf-api` (confirms it compiles with real signal wiring) then `cargo test -p serrf-api --bin serrf-api`
Expected: builds clean, both `wait_for_shutdown` tests still PASS

- [ ] **Step 6: Run the fast workspace suite plus fmt/clippy**

Run: `cargo test --workspace --lib --bins -- --skip golden` then `cargo fmt --check` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo clippy -p serrf-api --all-targets --features bundled-frontend -- -D warnings`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-api/Cargo.toml Cargo.lock crates/serrf-api/src/main.rs
git commit -m "Add graceful shutdown on Ctrl+C / SIGTERM"
```

---

### Task 3: SSE keep-alive

**Files:**
- Modify: `crates/serrf-api/src/routes/events.rs`
- Test: `crates/serrf-api/tests/events.rs` (modify — add one test)

**Interfaces:**
- Consumes: nothing new from earlier tasks.
- Produces: nothing exported; purely internal to the `/api/jobs/:id/events` handler.

**Context on the test's timing:** the keep-alive interval is set to 3 seconds in production code below (short enough to comfortably beat typical browser/proxy idle-connection timeouts of 30-60s, without being so aggressive it's chatty for a small local-network tool). To observe a keep-alive ping in a test, the job must still be running when the ping would fire — so the test below uses a larger fixture (80 compounds) than the other integration tests' 3-compound fixture, on the assumption that random-forest training over 80 compounds takes measurably longer than the compute needed to emit a keep-alive comment. If this specific fixture size doesn't reliably keep the job alive past ~3 seconds on your machine, increase the compound count (e.g., to 150 or 200) until it does — this is an explicitly authorized adjustment, not a plan deviation.

- [ ] **Step 1: Write the failing test**

Add to `crates/serrf-api/tests/events.rs`, after the existing `events_for_a_malformed_job_id_returns_400` test:

```rust
fn large_csv_fixture(compound_count: usize) -> String {
    let mut header = vec!["No".to_string(), "label".to_string()];
    let mut batch_row = vec!["".to_string(), "batch".to_string()];
    let mut type_row = vec!["".to_string(), "sampleType".to_string()];
    let mut time_row = vec!["".to_string(), "time".to_string()];
    for j in 0..20 {
        let is_qc = j < 12;
        let batch = if j % 4 < 2 { "A" } else { "B" };
        header.push(format!("s{j}"));
        batch_row.push(batch.to_string());
        type_row.push(if is_qc { "qc" } else { "sample" }.to_string());
        time_row.push(j.to_string());
    }
    let mut lines = vec![batch_row.join(","), type_row.join(","), time_row.join(","), header.join(",")];
    for i in 0..compound_count {
        let mut row = vec![(i + 1).to_string(), format!("Compound{i}")];
        for j in 0..20 {
            row.push((100.0 + i as f64 + j as f64 % 3.0).to_string());
        }
        lines.push(row.join(","));
    }
    lines.join("\n")
}

#[tokio::test]
async fn events_stream_includes_a_keep_alive_ping_while_a_long_job_runs() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().unwrap();
    let part = reqwest::multipart::Part::text(large_csv_fixture(80))
        .file_name("dataset.csv")
        .mime_str("text/csv")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    let response = client.get(format!("{base_url}/api/jobs/{job_id}/events")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();

    assert!(
        body.contains(": ") || body.lines().any(|line| line.starts_with(':')),
        "expected at least one SSE keep-alive comment line before the terminal event, got: {body}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-api --test events events_stream_includes_a_keep_alive_ping_while_a_long_job_runs`
Expected: FAIL — no keep-alive comment present, only `event: progress`/`event: completed` frames.

- [ ] **Step 3: Implement**

In `crates/serrf-api/src/routes/events.rs`, change the import line:

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
```

and change the final line of the `events` function from:

```rust
    Ok(Sse::new(stream))
```

to:

```rust
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(3))))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-api --test events events_stream_includes_a_keep_alive_ping_while_a_long_job_runs`
Expected: PASS. If it still fails because the 80-compound job completes before 3 seconds elapse, increase `large_csv_fixture(80)`'s argument (try 150, then 200) and re-run — this is the explicitly authorized adjustment mentioned above.

- [ ] **Step 5: Run the full events test file plus fmt/clippy**

Run: `cargo test -p serrf-api --test events` then `cargo fmt --check` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/serrf-api/src/routes/events.rs crates/serrf-api/tests/events.rs
git commit -m "Add SSE keep-alive pings to /api/jobs/:id/events"
```

---

### Task 4: Move CPU-bound work off async handlers

**Files:**
- Modify: `crates/serrf-api/src/routes/download.rs`
- Modify: `crates/serrf-api/src/routes/result.rs`
- Test: `crates/serrf-api/tests/blocking_handlers.rs` (create)

**Interfaces:**
- Consumes: `AppState.jobs: crate::job::JobStore` (existing, `Clone`).
- Produces: nothing exported; `download` and `result` keep their existing `pub async fn` signatures and route wiring in `app.rs` (unchanged).

- [ ] **Step 1: Write the failing test**

Create `crates/serrf-api/tests/blocking_handlers.rs`:

```rust
async fn spawn_app() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = serrf_api::app::build_app();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn valid_csv_fixture() -> String {
    let mut header = vec!["No".to_string(), "label".to_string()];
    let mut batch_row = vec!["".to_string(), "batch".to_string()];
    let mut type_row = vec!["".to_string(), "sampleType".to_string()];
    let mut time_row = vec!["".to_string(), "time".to_string()];
    for j in 0..20 {
        let is_qc = j < 12;
        let batch = if j % 4 < 2 { "A" } else { "B" };
        header.push(format!("s{j}"));
        batch_row.push(batch.to_string());
        type_row.push(if is_qc { "qc" } else { "sample" }.to_string());
        time_row.push(j.to_string());
    }
    let mut lines = vec![batch_row.join(","), type_row.join(","), time_row.join(","), header.join(",")];
    for i in 0..3 {
        let mut row = vec![(i + 1).to_string(), format!("Compound{i}")];
        for j in 0..20 {
            row.push((100.0 + i as f64 + j as f64 % 3.0).to_string());
        }
        lines.push(row.join(","));
    }
    lines.join("\n")
}

async fn upload_and_wait_for_completion(base_url: &str, client: &reqwest::Client) -> String {
    let part = reqwest::multipart::Part::text(valid_csv_fixture())
        .file_name("dataset.csv")
        .mime_str("text/csv")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    for _ in 0..200 {
        let status = client.get(format!("{base_url}/api/jobs/{job_id}")).send().await.unwrap();
        let body: serde_json::Value = status.json().await.unwrap();
        if body["status"] == "completed" || body["status"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    job_id
}

// `current_thread` is essential: with only one OS thread available for the whole runtime,
// a `/download` handler that runs its PCA + PNG-render + zip-build work synchronously
// (instead of via `spawn_blocking`, which hands it to tokio's separate blocking thread
// pool) would monopolize that single thread and starve every other request — including
// this test's concurrent `/health` request — until it finished.
#[tokio::test(flavor = "current_thread")]
async fn download_does_not_starve_a_concurrent_health_check() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let job_id = upload_and_wait_for_completion(&base_url, &client).await;

    let download_base_url = base_url.clone();
    let download_client = client.clone();
    let download_job_id = job_id.clone();
    tokio::spawn(async move {
        let _ = download_client
            .get(format!("{download_base_url}/api/jobs/{download_job_id}/download"))
            .send()
            .await;
    });

    // Give the spawned download request a moment to actually enter its handler.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let health = tokio::time::timeout(std::time::Duration::from_millis(500), client.get(format!("{base_url}/health")).send()).await;

    assert!(
        health.is_ok(),
        "a concurrent /health request should complete quickly even while /download is building its zip; \
         a timeout here means build_zip is running synchronously on the single-threaded runtime instead \
         of via spawn_blocking"
    );
    assert_eq!(health.unwrap().unwrap().status(), 200);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-api --test blocking_handlers`
Expected: FAIL (timeout) — `download`'s `build_zip` call currently runs synchronously inside the async handler, starving `/health` on the single-threaded test runtime. If it unexpectedly passes, `build_zip` for this tiny fixture may be finishing before the 10ms spawn delay; that would mean this test isn't exercising the codepath meaningfully — in that case, note it in your report as a concern (do not silently accept a passing "failing" test).

- [ ] **Step 3: Implement in `download.rs`**

In `crates/serrf-api/src/routes/download.rs`, replace the `download` function body (keep `build_zip` and everything below it unchanged):

```rust
pub async fn download(State(state): State<AppState>, Path(id): Path<String>) -> Result<impl IntoResponse, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;

    let jobs = state.jobs.clone();
    let lookup = tokio::task::spawn_blocking(move || jobs.with_completed(job_id, build_zip))
        .await
        .map_err(|e| ApiError::Internal(format!("download task panicked: {e}")))?
        .ok_or(ApiError::NotFound)?;

    let zip_bytes = match lookup {
        JobStoreLookup::Ready(bytes) => bytes.map_err(ApiError::Internal)?,
        JobStoreLookup::NotReady => return Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => return Err(ApiError::JobFailed(msg)),
    };

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"serrf-results.zip\""),
        ],
        zip_bytes,
    ))
}
```

- [ ] **Step 4: Implement in `result.rs`**

In `crates/serrf-api/src/routes/result.rs`, replace the `result` function body (keep the `PcaJson`/`ResultJson` struct definitions unchanged):

```rust
pub async fn result(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<ResultJson>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;

    let jobs = state.jobs.clone();
    let lookup = tokio::task::spawn_blocking(move || {
        jobs.with_completed(job_id, |completed| {
            // PCA excludes blank/None-sampleType columns entirely (app.R:1085-1086), matching
            // the report.png path in download.rs — otherwise the frontend renders an extra
            // "unknown" series for a group R never shows.
            let (raw_non_blank, pca_sample_type) = serrf_core::export::select_non_blank_columns(&completed.output.raw, &completed.sample_type);
            let pca_batch = serrf_core::export::select_non_blank_items(&completed.batch, &completed.sample_type);
            let sds_before: Vec<f64> = (0..raw_non_blank.nrows())
                .map(|i| serrf_core::export::std_dev(&raw_non_blank.row(i).to_vec()))
                .collect();
            let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&raw_non_blank, &sds_before));

            let (serrf_non_blank, _) = serrf_core::export::select_non_blank_columns(&completed.output.serrf, &completed.sample_type);
            let sds_after: Vec<f64> = (0..serrf_non_blank.nrows())
                .map(|i| serrf_core::export::std_dev(&serrf_non_blank.row(i).to_vec()))
                .collect();
            let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&serrf_non_blank, &sds_after));

            ResultJson {
                compound_labels: completed.compound_labels.clone(),
                qc_rsd_raw: completed.output.qc_rsd_raw.clone(),
                qc_rsd_serrf: completed.output.qc_rsd_serrf.clone(),
                validate_rsd_raw: completed.output.validate_rsd_raw.clone(),
                validate_rsd_serrf: completed.output.validate_rsd_serrf.clone(),
                pca_before: PcaJson {
                    pc1: pca_before.pc1,
                    pc2: pca_before.pc2,
                    sample_type: pca_sample_type.clone(),
                    batch: pca_batch.clone(),
                },
                pca_after: PcaJson {
                    pc1: pca_after.pc1,
                    pc2: pca_after.pc2,
                    sample_type: pca_sample_type,
                    batch: pca_batch,
                },
            }
        })
    })
    .await
    .map_err(|e| ApiError::Internal(format!("result task panicked: {e}")))?
    .ok_or(ApiError::NotFound)?;

    match lookup {
        JobStoreLookup::Ready(json) => Ok(Json(json)),
        JobStoreLookup::NotReady => Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => Err(ApiError::JobFailed(msg)),
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p serrf-api --test blocking_handlers`
Expected: PASS

- [ ] **Step 6: Run the full serrf-api test suite (excluding slow golden tests) plus fmt/clippy**

Run: `cargo test -p serrf-api -- --skip golden` then `cargo fmt --check` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo clippy -p serrf-api --all-targets --features bundled-frontend -- -D warnings`
Expected: all pass (this exercises the existing `tests/download.rs` and `tests/status.rs` files too — confirm no regressions)

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-api/src/routes/download.rs crates/serrf-api/src/routes/result.rs crates/serrf-api/tests/blocking_handlers.rs
git commit -m "Move download/result CPU-bound work off the async runtime via spawn_blocking"
```

---

### Task 5: JobStore TTL eviction

**Files:**
- Modify: `crates/serrf-api/src/job.rs`
- Modify: `crates/serrf-api/src/app.rs`

**Interfaces:**
- Consumes: `app.rs` as left by Task 1 (three-layer chain ending `.with_state(state);`).
- Produces: `JobStore::evict_expired(&self, older_than: std::time::Duration)` — a new public method other tasks do not need but future work may reuse.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/serrf-api/src/job.rs`, after the existing `with_completed_reports_not_found_for_an_unknown_job` test:

```rust
    #[test]
    fn evict_expired_removes_a_job_completed_before_the_cutoff() {
        let store = JobStore::new();
        let (id, _rx) = store.create();
        store.complete(id, sample_completed());
        std::thread::sleep(std::time::Duration::from_millis(20));

        store.evict_expired(std::time::Duration::from_millis(5));

        assert!(store.subscribe(id).is_none());
    }

    #[test]
    fn evict_expired_keeps_a_job_completed_after_the_cutoff() {
        let store = JobStore::new();
        let (id, _rx) = store.create();
        store.complete(id, sample_completed());

        store.evict_expired(std::time::Duration::from_secs(60));

        assert!(store.subscribe(id).is_some());
    }

    #[test]
    fn evict_expired_never_removes_a_job_that_has_not_finished() {
        let store = JobStore::new();
        let (id, _rx) = store.create();

        store.evict_expired(std::time::Duration::from_secs(0));

        assert!(store.subscribe(id).is_some());
    }

    #[test]
    fn evict_expired_removes_a_failed_job_completed_before_the_cutoff() {
        let store = JobStore::new();
        let (id, _rx) = store.create();
        store.fail(id, "boom".to_string());
        std::thread::sleep(std::time::Duration::from_millis(20));

        store.evict_expired(std::time::Duration::from_millis(5));

        assert!(store.subscribe(id).is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-api --lib job::tests`
Expected: FAIL to compile — `JobStore::evict_expired` does not exist yet.

- [ ] **Step 3: Implement**

In `crates/serrf-api/src/job.rs`:

Add a `completed_at` field to `JobHandle`:

```rust
struct JobHandle {
    events: tokio::sync::watch::Sender<JobEvent>,
    result: JobResult,
    completed_at: Option<std::time::Instant>,
}
```

In `JobStore::create`, initialize the new field:

```rust
    pub fn create(&self) -> (JobId, tokio::sync::watch::Receiver<JobEvent>) {
        let id = JobId::new();
        let (tx, rx) = tokio::sync::watch::channel(JobEvent::Queued);
        self.jobs.write().unwrap().insert(
            id,
            JobHandle {
                events: tx,
                result: JobResult::Pending,
                completed_at: None,
            },
        );
        (id, rx)
    }
```

In `JobStore::complete`, set the timestamp:

```rust
    pub fn complete(&self, id: JobId, completed: CompletedJob) {
        if let Some(handle) = self.jobs.write().unwrap().get_mut(&id) {
            handle.result = JobResult::Done(Box::new(completed));
            handle.completed_at = Some(std::time::Instant::now());
            let _ = handle.events.send_replace(JobEvent::Completed);
        }
    }
```

In `JobStore::fail`, set the timestamp:

```rust
    pub fn fail(&self, id: JobId, error: String) {
        if let Some(handle) = self.jobs.write().unwrap().get_mut(&id) {
            let _ = handle.events.send_replace(JobEvent::Failed { error: error.clone() });
            handle.result = JobResult::Errored(error);
            handle.completed_at = Some(std::time::Instant::now());
        }
    }
```

Add the new method after `with_completed`:

```rust
    /// Drops any job whose terminal (Completed/Failed) event happened more than `older_than`
    /// ago. A job that hasn't finished yet (`completed_at` is `None`) is never evicted.
    pub fn evict_expired(&self, older_than: std::time::Duration) {
        self.jobs.write().unwrap().retain(|_, handle| match handle.completed_at {
            Some(completed_at) => completed_at.elapsed() < older_than,
            None => true,
        });
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-api --lib job::tests`
Expected: PASS (all 4 new tests plus the existing 7)

- [ ] **Step 5: Wire a periodic sweep into `build_app`**

In `crates/serrf-api/src/app.rs`, at the top of `build_app`, after the `state` is created but before the router is built, spawn a background sweep task:

```rust
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
```

(Everything from `.route("/health", ...)` onward in `build_app` is unchanged.)

- [ ] **Step 6: Run the fast workspace suite plus fmt/clippy**

Run: `cargo test --workspace --lib --bins -- --skip golden` then `cargo fmt --check` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo clippy -p serrf-api --all-targets --features bundled-frontend -- -D warnings`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-api/src/job.rs crates/serrf-api/src/app.rs
git commit -m "Evict completed/failed jobs from JobStore after 30 minutes"
```

---

### Task 6: Document the CORS/bind posture

**Files:**
- Modify: `crates/serrf-api/src/app.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes/produces: nothing — comment-only and documentation-only change.

- [ ] **Step 1: Add a code comment**

In `crates/serrf-api/src/app.rs`, immediately above the `.layer(tower_http::cors::CorsLayer::permissive())` line, add:

```rust
        // Deliberately permissive: this API has no authentication anywhere (by design — see
        // README.md's "Security posture" section) and is meant to run on a single host behind
        // a reverse proxy or as the bundled-frontend standalone exe, not exposed multi-tenant.
        // Restricting the origin here would add friction without adding real security, since
        // there's no auth boundary for CORS to protect in the first place.
        .layer(tower_http::cors::CorsLayer::permissive())
```

- [ ] **Step 2: Add a README section**

In `README.md`, insert a new `## Security posture` section immediately after the `## Status` section (before `## Running the Rust CLI`):

```markdown
## Security posture

`serrf-api` has no authentication and uses a permissive CORS policy
(`CorsLayer::permissive()`). This is deliberate: it's a single-host research tool with no
multi-tenant use case, meant to run behind a reverse proxy (or as the bundled-frontend
standalone executable, which binds to `127.0.0.1` only — see below) rather than exposed
directly to untrusted networks. In the non-bundled deployment mode, `serrf-api` binds
`0.0.0.0` because that's required for Docker's port mapping to reach it; this is not a
security boundary on its own and assumes the container itself is not exposed to an
untrusted network. If this tool is ever deployed multi-tenant or on an untrusted network,
add real authentication first — restricting CORS alone would not meaningfully help.
```

- [ ] **Step 3: Verify**

Run: `cargo check -p serrf-api` then `cargo fmt --check`
Expected: both pass (comment-only change, no behavior to test)

- [ ] **Step 4: Commit**

```bash
git add crates/serrf-api/src/app.rs README.md
git commit -m "Document the deliberate permissive-CORS / no-auth posture"
```

---

### Task 7: Remove the dead `errorMessage` prop from `UploadForm`

**Files:**
- Modify: `frontend/src/components/UploadForm.tsx`
- Modify: `frontend/src/components/UploadForm.test.tsx`

**Interfaces:**
- Consumes: nothing.
- Produces: `UploadForm`'s prop type shrinks to `{ onSubmit: (file: File) => void }`. `frontend/src/app/page.tsx` already only ever calls `<UploadForm onSubmit={submit} />` (no `errorMessage`), so no caller needs updating.

- [ ] **Step 1: Write the failing test (by deleting the test that covers dead behavior)**

In `frontend/src/components/UploadForm.test.tsx`, remove the second test case (`"shows an error message when provided"`) so the file reads:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UploadForm from "./UploadForm";

describe("UploadForm", () => {
  it("disables submit until a file is chosen, then calls onSubmit with it", async () => {
    const onSubmit = vi.fn();
    render(<UploadForm onSubmit={onSubmit} />);

    const button = screen.getByRole("button", { name: /run serrf normalization/i });
    expect(button).toBeDisabled();

    const file = new File(["a,b\n1,2"], "dataset.csv", { type: "text/csv" });
    await userEvent.upload(screen.getByLabelText(/dataset file/i), file);
    expect(button).toBeEnabled();

    await userEvent.click(button);

    expect(onSubmit).toHaveBeenCalledWith(file);
  });
});
```

This step is a deletion, not an addition, so there is no "watch it fail" moment in the usual TDD sense — the goal is to remove test coverage for a feature we are about to delete, before deleting the feature it tests, so the test suite never has a moment where it's asserting something that no longer exists.

- [ ] **Step 2: Run test to verify the remaining test still passes**

Run: `cd frontend && npm test -- UploadForm`
Expected: PASS (1 test, the deleted one is gone)

- [ ] **Step 3: Remove the prop from the component**

Replace `frontend/src/components/UploadForm.tsx` in full:

```tsx
"use client";

import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";

interface UploadFormProps {
  onSubmit: (file: File) => void;
}

export default function UploadForm({ onSubmit }: UploadFormProps) {
  const [file, setFile] = useState<File | null>(null);

  return (
    <Box
      component="form"
      onSubmit={(event) => {
        event.preventDefault();
        if (file) {
          onSubmit(file);
        }
      }}
    >
      <Typography variant="h5" gutterBottom>
        Upload a dataset
      </Typography>
      <input
        type="file"
        accept=".csv,.xlsx"
        aria-label="dataset file"
        onChange={(event) => setFile(event.target.files?.[0] ?? null)}
      />
      <Box sx={{ mt: 2 }}>
        <Button type="submit" variant="contained" disabled={!file}>
          Run SERRF normalization
        </Button>
      </Box>
    </Box>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npm test -- UploadForm`
Expected: PASS

- [ ] **Step 5: Run lint, typecheck, and the full frontend unit suite**

Run: `cd frontend && npm run lint && npm run typecheck && npm test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/UploadForm.tsx frontend/src/components/UploadForm.test.tsx
git commit -m "Remove dead errorMessage prop from UploadForm"
```

---

### Task 8: Fix theme flash on first paint

**Files:**
- Modify: `frontend/src/app/layout.tsx`
- Modify: `frontend/src/app/ThemeRegistry.tsx`
- Test: `frontend/src/app/ThemeRegistry.test.tsx` (create)

**Interfaces:**
- Consumes: nothing.
- Produces: `document.documentElement.dataset.colorMode` becomes a contract between the inline script in `layout.tsx` and `ThemeRegistry`'s initial-state reader — no other task touches this.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/app/ThemeRegistry.test.tsx`:

```tsx
import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { useContext } from "react";
import ThemeRegistry, { ColorModeContext } from "./ThemeRegistry";

function ModeProbe() {
  const { mode } = useContext(ColorModeContext);
  return <div data-testid="mode">{mode}</div>;
}

describe("ThemeRegistry", () => {
  afterEach(() => {
    delete document.documentElement.dataset.colorMode;
  });

  it("picks up the color mode already stamped on <html> by the pre-hydration script, without waiting for an effect", () => {
    document.documentElement.dataset.colorMode = "dark";

    render(
      <ThemeRegistry>
        <ModeProbe />
      </ThemeRegistry>
    );

    expect(screen.getByTestId("mode")).toHaveTextContent("dark");
  });

  it("defaults to light when no color mode has been stamped on <html>", () => {
    render(
      <ThemeRegistry>
        <ModeProbe />
      </ThemeRegistry>
    );

    expect(screen.getByTestId("mode")).toHaveTextContent("light");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npm test -- ThemeRegistry`
Expected: FAIL — the first test fails because `ThemeRegistry` currently always starts at `"light"` and only reads `localStorage`/`matchMedia` in a post-mount `useEffect`, never reading `document.documentElement.dataset.colorMode`.

- [ ] **Step 3: Implement the synchronous initial-mode read in `ThemeRegistry`**

Replace `frontend/src/app/ThemeRegistry.tsx` in full:

```tsx
"use client";

import { createContext, useMemo, useState } from "react";
import CssBaseline from "@mui/material/CssBaseline";
import { ThemeProvider } from "@mui/material/styles";
import { AppRouterCacheProvider } from "@mui/material-nextjs/v14-appRouter";
import { getTheme } from "./theme";

export type ColorMode = "light" | "dark";

export const ColorModeContext = createContext<{ mode: ColorMode; toggle: () => void }>({
  mode: "light",
  toggle: () => {},
});

function getInitialMode(): ColorMode {
  // The blocking inline script in layout.tsx's <head> (COLOR_MODE_INIT_SCRIPT) runs before
  // hydration and stamps this attribute, so reading it here — rather than always starting at
  // "light" and correcting in a useEffect after mount — avoids a flash of the wrong theme.
  if (typeof document === "undefined") {
    return "light";
  }
  return document.documentElement.dataset.colorMode === "dark" ? "dark" : "light";
}

export default function ThemeRegistry({ children }: { children: React.ReactNode }) {
  const [mode, setMode] = useState<ColorMode>(getInitialMode);

  const contextValue = useMemo(
    () => ({
      mode,
      toggle: () => {
        setMode((current) => {
          const next = current === "light" ? "dark" : "light";
          localStorage.setItem("color-mode", next);
          document.documentElement.dataset.colorMode = next;
          return next;
        });
      },
    }),
    [mode]
  );

  const theme = useMemo(() => getTheme(mode), [mode]);

  return (
    <AppRouterCacheProvider>
      <ColorModeContext.Provider value={contextValue}>
        <ThemeProvider theme={theme}>
          <CssBaseline />
          {children}
        </ThemeProvider>
      </ColorModeContext.Provider>
    </AppRouterCacheProvider>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npm test -- ThemeRegistry`
Expected: PASS

- [ ] **Step 5: Add the pre-hydration blocking script to `layout.tsx`**

Replace `frontend/src/app/layout.tsx` in full:

```tsx
import type { Metadata } from "next";
import ThemeRegistry from "./ThemeRegistry";
import "./globals.css";

export const metadata: Metadata = {
  title: "RustySERRF",
  description: "SERRF normalization for metabolomics data",
};

// Runs before hydration to stamp document.documentElement with the persisted (or OS-preferred)
// color mode and paint matching MUI-default colors immediately, so ThemeRegistry's first client
// render — which reads the same attribute via getInitialMode() — never has to correct a
// wrongly-guessed initial theme after the fact.
const COLOR_MODE_INIT_SCRIPT = `(function () {
  try {
    var stored = localStorage.getItem("color-mode");
    var mode = stored === "light" || stored === "dark"
      ? stored
      : (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
    document.documentElement.dataset.colorMode = mode;
    document.documentElement.style.colorScheme = mode;
    document.documentElement.style.backgroundColor = mode === "dark" ? "#121212" : "#fff";
    document.documentElement.style.color = mode === "dark" ? "#fff" : "rgba(0, 0, 0, 0.87)";
  } catch (e) {}
})();`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <script dangerouslySetInnerHTML={{ __html: COLOR_MODE_INIT_SCRIPT }} />
      </head>
      <body>
        <ThemeRegistry>{children}</ThemeRegistry>
      </body>
    </html>
  );
}
```

(The color values `#121212` / `#fff` / `rgba(0, 0, 0, 0.87)` are MUI's own default dark/light `background.default` and `text.primary` values — matching them exactly means once React hydrates and `CssBaseline` applies its own styles, there is no visible color jump.)

- [ ] **Step 6: Run lint, typecheck, and the full frontend unit suite**

Run: `cd frontend && npm run lint && npm run typecheck && npm test`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add frontend/src/app/layout.tsx frontend/src/app/ThemeRegistry.tsx frontend/src/app/ThemeRegistry.test.tsx
git commit -m "Fix theme flash on first paint via a pre-hydration inline script"
```

---

### Task 9: Fix `RsdBarChart` at real compound-count scale

**Files:**
- Modify: `frontend/src/components/RsdBarChart.tsx`
- Modify: `frontend/src/components/RsdBarChart.test.tsx`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing exported beyond the existing `RsdBarChart` component and its existing prop shape (unchanged).

- [ ] **Step 1: Write the failing test**

Add to `frontend/src/components/RsdBarChart.test.tsx`, after the existing test:

```tsx
  it("gives the chart enough width to stay readable at real compound counts, inside a horizontally scrollable container", () => {
    const compoundLabels = Array.from({ length: 268 }, (_, i) => `c${i}`);
    const values = compoundLabels.map(() => 0.1);
    const { container } = render(<RsdBarChart compoundLabels={compoundLabels} qcRsdRaw={values} qcRsdSerrf={values} />);

    const svg = container.querySelector("svg");
    expect(svg).toBeInTheDocument();
    const svgWidth = Number(svg?.getAttribute("width"));
    expect(svgWidth).toBeGreaterThanOrEqual(268 * 20);

    const scrollContainer = container.firstElementChild as HTMLElement;
    expect(scrollContainer.style.overflowX).toBe("auto");
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npm test -- RsdBarChart`
Expected: FAIL — the current `BarChart` has no explicit `width` (so it renders at whatever its ResizeObserver-measured container width is, roughly the jsdom default of a few hundred pixels, far less than `268 * 20`) and no scrollable wrapper exists.

- [ ] **Step 3: Implement**

Replace `frontend/src/components/RsdBarChart.tsx` in full:

```tsx
import Box from "@mui/material/Box";
import { BarChart } from "@mui/x-charts/BarChart";

interface RsdBarChartProps {
  compoundLabels: string[];
  qcRsdRaw: (number | null)[];
  qcRsdSerrf: (number | null)[];
}

const PIXELS_PER_COMPOUND = 24;
const MIN_CHART_WIDTH = 600;

export default function RsdBarChart({ compoundLabels, qcRsdRaw, qcRsdSerrf }: RsdBarChartProps) {
  const width = Math.max(MIN_CHART_WIDTH, compoundLabels.length * PIXELS_PER_COMPOUND);

  return (
    <Box sx={{ overflowX: "auto" }}>
      <BarChart
        width={width}
        height={400}
        xAxis={[{ scaleType: "band", data: compoundLabels, label: "Compound" }]}
        yAxis={[{ label: "QC-RSD" }]}
        series={[
          { data: qcRsdRaw, label: "Raw QC-RSD" },
          { data: qcRsdSerrf, label: "SERRF QC-RSD" },
        ]}
      />
    </Box>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npm test -- RsdBarChart`
Expected: PASS. If the `svg[width]` assertion fails because MUI X Charts v9 exposes the rendered width differently than a plain `width` attribute on the root `<svg>`, inspect the actual rendered DOM (`screen.debug()` or `container.innerHTML`) to find the correct selector/attribute and adjust the test's assertion accordingly — the intent (chart renders far wider than its viewport, inside a scrollable box) is what matters, not the exact DOM query.

- [ ] **Step 5: Run lint, typecheck, and the full frontend unit suite**

Run: `cd frontend && npm run lint && npm run typecheck && npm test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/RsdBarChart.tsx frontend/src/components/RsdBarChart.test.tsx
git commit -m "Fix RsdBarChart becoming unreadable at real compound counts"
```

---

### Task 10: Style the page heading through the theme

**Files:**
- Modify: `frontend/src/app/page.tsx`
- Modify: `frontend/src/app/page.test.tsx`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing exported; `page.tsx`'s rendered heading is still an `<h1>` (same accessible role/name), just theme-styled.

- [ ] **Step 1: Write the failing test**

Add to `frontend/src/app/page.test.tsx`, inside the existing `describe("Home", ...)` block, after the first test (`"renders the app title and starts in the upload state"`):

```tsx
  it("styles the heading through the theme instead of an unstyled raw <h1>", () => {
    mockJobState({ phase: "idle" });
    render(<Home />);

    const heading = screen.getByRole("heading", { name: "RustySERRF" });
    expect(heading.tagName).toBe("H1");
    expect(heading.className).toMatch(/Mui/);
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npm test -- page.test`
Expected: FAIL — the current `<h1>RustySERRF</h1>` is a raw HTML element with no `className` at all, so `.toMatch(/Mui/)` fails.

- [ ] **Step 3: Implement**

In `frontend/src/app/page.tsx`, add the import (alongside the other MUI imports at the top):

```tsx
import Typography from "@mui/material/Typography";
```

and change:

```tsx
        <h1>RustySERRF</h1>
```

to:

```tsx
        <Typography variant="h4" component="h1">
          RustySERRF
        </Typography>
```

(`component="h1"` keeps the semantic/accessible tag and heading level identical to before; `variant="h4"` gives it theme-driven color/typography at a size close to the original browser-default `<h1>` rendering, rather than MUI's much larger default `h1` variant size.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npm test -- page.test`
Expected: PASS (both the new test and the pre-existing `getByRole("heading", { name: "RustySERRF" })` assertion in the first test)

- [ ] **Step 5: Run lint, typecheck, and the full frontend unit suite**

Run: `cd frontend && npm run lint && npm run typecheck && npm test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add frontend/src/app/page.tsx frontend/src/app/page.test.tsx
git commit -m "Style the page heading through MUI Typography instead of a raw <h1>"
```

---

### Task 11: Special-case 413 (payload too large) upload errors

**Files:**
- Modify: `frontend/src/lib/api.ts`
- Modify: `frontend/src/lib/api.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing exported beyond `parseErrorMessage`'s existing (private) behavior — callers of `uploadDataset`/`fetchJobStatus`/`fetchJobResult` see no signature change, just a friendlier message on a 413.

- [ ] **Step 1: Write the failing test**

Add to `frontend/src/lib/api.test.ts`, inside the `describe("uploadDataset", ...)` block (find it by searching for that describe block; add after its existing tests, before the closing `});` of that describe):

```ts
  it("shows a friendly message for a 413 response instead of the raw status text", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 413,
        statusText: "Payload Too Large",
        json: async () => {
          throw new Error("no JSON body on a 413");
        },
      })
    );

    const file = new File(["a,b\n1,2"], "dataset.csv", { type: "text/csv" });
    await expect(uploadDataset(file)).rejects.toMatchObject(new ApiError("File is too large (max 10MB).", 413));
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npm test -- api.test`
Expected: FAIL — `parseErrorMessage` currently falls back to `response.statusText` ("Payload Too Large") on any non-JSON error body, not the friendly message.

- [ ] **Step 3: Implement**

In `frontend/src/lib/api.ts`, replace the `parseErrorMessage` function:

```ts
async function parseErrorMessage(response: Response): Promise<string> {
  if (response.status === 413) {
    return "File is too large (max 10MB).";
  }
  try {
    const body = (await response.json()) as { error?: string };
    return typeof body.error === "string" ? body.error : response.statusText;
  } catch {
    return response.statusText;
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npm test -- api.test`
Expected: PASS

- [ ] **Step 5: Run lint, typecheck, and the full frontend unit suite**

Run: `cd frontend && npm run lint && npm run typecheck && npm test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/api.ts frontend/src/lib/api.test.ts
git commit -m "Show a friendly message for 413 upload errors instead of raw status text"
```

---

## After all tasks

Run the full workspace + frontend + e2e suites once (matching CI exactly):

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p serrf-api --all-targets --features bundled-frontend -- -D warnings
cargo test --workspace
cargo test -p serrf-api --features bundled-frontend
cargo build --workspace --release
cd frontend && npm run lint && npm run typecheck && npm run test:coverage && npm run build
```

Then proceed to the final whole-branch review per subagent-driven-development, and ship via the same push → PR → CI → merge flow used for every other change in this repo this session.
