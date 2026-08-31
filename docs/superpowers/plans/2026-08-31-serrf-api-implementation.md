# serrf-api Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `serrf-api`, an axum HTTP server that wraps `serrf-core`'s pipeline behind an async job model — upload a dataset, poll/stream progress over SSE, fetch JSON results, download a CSV+PNG zip — with no database, matching the existing R/Shiny app's upload→normalize→download flow.

**Architecture:** A new `crates/serrf-api` binary crate depending only on `serrf-core` (never the reverse). An in-memory `JobStore` (`Arc<RwLock<HashMap<JobId, JobHandle>>>`) tracks job status; each job runs `serrf_core::normalize` on a `tokio::task::spawn_blocking` thread (it's a synchronous, CPU-bound, internally-rayon-parallel call — it must never run directly on an async task, which would starve the executor). Progress flows from the blocking thread to SSE clients via a `tokio::sync::watch` channel per job (a "latest value wins" channel — exactly right for a progress bar, no backpressure concerns). Before any of that, the CSV-writing helpers currently private to `serrf-cli` move into `serrf-core` as a shared, `Write`-generic `export` module, since `serrf-api`'s download endpoint needs identical output.

**Tech Stack:** `axum` 0.7 (HTTP, multipart, SSE), `tokio` 1.x (`spawn_blocking` for the pipeline, async runtime), `serde`/`serde_json` (JSON responses), `uuid` (job ids), `tempfile` (uploaded file + rendered PNG staging), `async-stream` (SSE stream construction), `zip` (download archive), `reqwest` (dev-dependency, real HTTP integration tests), `thiserror` (error types).

**Spec:** `docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md` — this plan implements that spec's "Data flow" section (routes, job model, SSE granularity) and "Testing strategy" section (real HTTP integration tests against a running axum instance) for `serrf-api` specifically. Plan 1 (`serrf-core`+`serrf-cli`, already merged to `master`) built everything this plan depends on.

## Global Constraints

- TDD non-negotiable: write the failing test before implementation code, for every task below.
- No mocks/stubs/fakes — integration tests make real HTTP requests against a real running axum instance on an ephemeral port (`reqwest` client), real multipart uploads, real file I/O via `tempfile`.
- 80%+ coverage (statements, branches, functions, lines) for `serrf-api` — check with `cargo tarpaulin` before considering the plan done.
- Commit after every task (not every step) — one focused commit per task, on branch `feat/serrf-api` (create this branch from the tip of `master` before Task 1 — Plan 1 is already merged there).
- `serrf-core` must never depend on `serrf-api`, `axum`, `tokio`, or any HTTP/job-lifecycle concept — the split from the spec is `serrf-core` = pure algorithm, `serrf-api` = HTTP/job wrapper. The one exception is Task 1's `export` module, which is pure `Write`-generic CSV serialization — no HTTP, no async, no job concepts.
- `serrf_core::normalize` is synchronous and CPU-bound (internally parallel via `rayon`) — every call to it in `serrf-api` MUST run inside `tokio::task::spawn_blocking`, never directly in an `async fn` handler or a plain `tokio::spawn`.
- Out of scope for this plan (per spec): no database/persistent job history, no auth/multi-user, no Docker/deployment scripts (`dev-start.sh` etc. — Plan 4), no frontend (Plan 3). A job lost on server restart is acceptable, per spec.
- Integration tests that exercise the real bundled 1299-sample/268-compound example dataset are slow (~5 min) and belong in their own test file (Task 9) so the fast test suite (Tasks 2-8) stays fast for TDD iteration; all other integration tests use small synthetic fixtures that complete in well under a second.

---

### Task 1: Move CSV-export helpers from `serrf-cli` into `serrf-core` as a shared, writer-generic module

**Files:**
- Create: `crates/serrf-core/src/export.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod export;`)
- Modify: `crates/serrf-cli/src/main.rs` (delegate to `serrf_core::export`, remove the now-duplicated private functions)

**Interfaces:**
- Produces: `serrf_core::export::write_matrix_csv<W: std::io::Write>(writer: W, sample_labels: &[String], compound_labels: &[String], matrix: &ndarray::Array2<f64>) -> Result<(), serrf_core::error::SerrfError>`, `serrf_core::export::write_rsd_csv<W: std::io::Write>(writer: W, labels: &[String], raw: &[f64], serrf: &[f64], validate_rsd_raw: &std::collections::HashMap<String, Vec<f64>>, validate_rsd_serrf: &std::collections::HashMap<String, Vec<f64>>) -> Result<(), serrf_core::error::SerrfError>`, `serrf_core::export::std_dev(values: &[f64]) -> f64`, `serrf_core::export::filter_rows_with_variance(matrix: &ndarray::Array2<f64>, sds: &[f64]) -> ndarray::Array2<f64>`.
- Consumes (Task 4 onward in `serrf-api`): the same four functions, used identically to how `serrf-cli` uses them today, but writing into a zip entry instead of a file.

- [ ] **Step 1: Write the failing tests in the new module**

Create `crates/serrf-core/src/export.rs` with just the test module first (no implementation), adapted from `serrf-cli`'s existing tests to use `Vec<u8>` as the writer:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;
    use std::collections::HashMap;

    #[test]
    fn std_dev_matches_the_hand_computed_sample_standard_deviation() {
        let result = std_dev(&[1.0, 2.0, 3.0]);
        assert!((result - 1.0).abs() < 1e-12, "expected 1.0, got {result}");
    }

    #[test]
    fn std_dev_of_a_constant_series_is_zero() {
        assert_eq!(std_dev(&[5.0, 5.0, 5.0, 5.0]), 0.0);
    }

    #[test]
    fn filter_rows_with_variance_drops_rows_with_zero_sd() {
        let matrix = array![[1.0, 2.0], [3.0, 3.0], [4.0, 6.0]];
        let sds = [1.0, 0.0, 2.5];
        let filtered = filter_rows_with_variance(&matrix, &sds);
        assert_eq!(filtered.shape(), &[2, 2]);
        assert_eq!(filtered.row(0).to_vec(), vec![1.0, 2.0]);
        assert_eq!(filtered.row(1).to_vec(), vec![4.0, 6.0]);
    }

    #[test]
    fn filter_rows_with_variance_keeps_everything_when_all_sds_are_positive() {
        let matrix = array![[1.0, 2.0], [3.0, 4.0]];
        let sds = [0.5, 0.7];
        let filtered = filter_rows_with_variance(&matrix, &sds);
        assert_eq!(filtered.shape(), matrix.shape());
    }

    #[test]
    fn write_matrix_csv_writes_a_header_using_the_real_sample_labels() {
        let matrix = array![[1.5, 2.5], [3.5, 4.5]];
        let sample_labels = vec!["QC001".to_string(), "GB00042".to_string()];
        let compound_labels = vec!["c1".to_string(), "c2".to_string()];
        let mut buf = Vec::new();
        write_matrix_csv(&mut buf, &sample_labels, &compound_labels, &matrix).unwrap();

        let content = String::from_utf8(buf).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC001,GB00042");
        assert_eq!(lines.next().unwrap(), "c1,1.5,2.5");
        assert_eq!(lines.next().unwrap(), "c2,3.5,4.5");
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_rsd_csv_writes_a_header_and_one_row_per_label() {
        let labels = vec!["c1".to_string(), "c2".to_string()];
        let mut buf = Vec::new();
        write_rsd_csv(&mut buf, &labels, &[0.1, 0.2], &[0.01, 0.02], &HashMap::new(), &HashMap::new()).unwrap();

        let content = String::from_utf8(buf).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC_none,QC_SERRF");
        assert_eq!(lines.next().unwrap(), "c1,0.1,0.01");
        assert_eq!(lines.next().unwrap(), "c2,0.2,0.02");
        assert!(lines.next().is_none());
    }

    #[test]
    fn write_rsd_csv_adds_validate_columns_when_present() {
        let labels = vec!["c1".to_string(), "c2".to_string()];
        let mut validate_raw = HashMap::new();
        validate_raw.insert("validate".to_string(), vec![0.3, 0.4]);
        let mut validate_serrf = HashMap::new();
        validate_serrf.insert("validate".to_string(), vec![0.03, 0.04]);
        let mut buf = Vec::new();
        write_rsd_csv(&mut buf, &labels, &[0.1, 0.2], &[0.01, 0.02], &validate_raw, &validate_serrf).unwrap();

        let content = String::from_utf8(buf).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "label,QC_none,QC_SERRF,validate_none,validate_SERRF");
        assert_eq!(lines.next().unwrap(), "c1,0.1,0.01,0.3,0.03");
        assert_eq!(lines.next().unwrap(), "c2,0.2,0.02,0.4,0.04");
        assert!(lines.next().is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p serrf-core --lib export::`
Expected: FAIL to compile — `std_dev`, `filter_rows_with_variance`, `write_matrix_csv`, `write_rsd_csv` not found in this scope.

- [ ] **Step 3: Add `pub mod export;` to `crates/serrf-core/src/lib.rs`**

Add the line `pub mod export;` alongside the other `pub mod` declarations.

- [ ] **Step 4: Implement the module above the test block in `crates/serrf-core/src/export.rs`**

```rust
use crate::error::SerrfError;
use std::collections::HashMap;
use std::io::Write;

pub fn std_dev(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() as f64 - 1.0)).sqrt()
}

pub fn filter_rows_with_variance(matrix: &ndarray::Array2<f64>, sds: &[f64]) -> ndarray::Array2<f64> {
    let keep: Vec<usize> = (0..sds.len()).filter(|&i| sds[i] > 0.0).collect();
    matrix.select(ndarray::Axis(0), &keep)
}

pub fn write_matrix_csv<W: Write>(
    writer: W,
    sample_labels: &[String],
    compound_labels: &[String],
    matrix: &ndarray::Array2<f64>,
) -> Result<(), SerrfError> {
    let mut writer = csv::Writer::from_writer(writer);
    writer.write_record(std::iter::once("label".to_string()).chain(sample_labels.iter().cloned()))?;
    for (i, label) in compound_labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        row.extend(matrix.row(i).iter().map(|v| v.to_string()));
        writer.write_record(&row)?;
    }
    writer.flush().map_err(SerrfError::Io)?;
    Ok(())
}

pub fn write_rsd_csv<W: Write>(
    writer: W,
    labels: &[String],
    raw: &[f64],
    serrf: &[f64],
    validate_rsd_raw: &HashMap<String, Vec<f64>>,
    validate_rsd_serrf: &HashMap<String, Vec<f64>>,
) -> Result<(), SerrfError> {
    let mut writer = csv::Writer::from_writer(writer);
    let mut validate_types: Vec<&String> = validate_rsd_raw.keys().collect();
    validate_types.sort();

    let mut header = vec!["label".to_string(), "QC_none".to_string(), "QC_SERRF".to_string()];
    for t in &validate_types {
        header.push(format!("{t}_none"));
        header.push(format!("{t}_SERRF"));
    }
    writer.write_record(&header)?;

    for (i, label) in labels.iter().enumerate() {
        let mut row = vec![label.clone(), raw[i].to_string(), serrf[i].to_string()];
        for t in &validate_types {
            row.push(validate_rsd_raw[*t][i].to_string());
            row.push(validate_rsd_serrf[*t][i].to_string());
        }
        writer.write_record(&row)?;
    }
    writer.flush().map_err(SerrfError::Io)?;
    Ok(())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p serrf-core --lib export::`
Expected: PASS, 6 tests.

- [ ] **Step 6: Refactor `serrf-cli` to use the shared module, deleting its now-duplicate private functions**

In `crates/serrf-cli/src/main.rs`: delete the `std_dev`, `filter_rows_with_variance`, `write_matrix_csv`, `write_rsd_csv` function definitions and their unit tests (moved to Task 1's `export.rs`). Replace call sites:

```rust
let sds_before: Vec<f64> = (0..dataset.values.nrows())
    .map(|i| serrf_core::export::std_dev(&output.raw.row(i).to_vec()))
    .collect();
let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&output.raw, &sds_before));
let sds_after: Vec<f64> = (0..dataset.values.nrows())
    .map(|i| serrf_core::export::std_dev(&output.serrf.row(i).to_vec()))
    .collect();
let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&output.serrf, &sds_after));
```

And replace the `write_matrix_csv`/`write_rsd_csv` call sites' bodies with `std::fs::File::create(path)?` passed as the writer:

```rust
fn write_matrix_csv(path: &std::path::Path, sample_labels: &[String], compound_labels: &[String], matrix: &ndarray::Array2<f64>) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    serrf_core::export::write_matrix_csv(file, sample_labels, compound_labels, matrix)?;
    Ok(())
}

fn write_rsd_csv(
    path: &std::path::Path,
    labels: &[String],
    raw: &[f64],
    serrf: &[f64],
    validate_rsd_raw: &HashMap<String, Vec<f64>>,
    validate_rsd_serrf: &HashMap<String, Vec<f64>>,
) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    serrf_core::export::write_rsd_csv(file, labels, raw, serrf, validate_rsd_raw, validate_rsd_serrf)?;
    Ok(())
}
```

Keep `write_matrix_csv`/`write_rsd_csv` as thin file-opening wrappers in `serrf-cli` (its own tests for these two wrapper functions, which assert file contents on disk, stay as-is and keep passing — they now exercise the shared module through the wrapper).

- [ ] **Step 7: Run the full fast test suite to verify nothing broke**

Run: `cargo test -p serrf-core --lib && cargo test -p serrf-cli --lib`
Expected: PASS, no failures. (`serrf-cli`'s `tests/cli.rs` integration test is slow — skip it here, Task 9 of this plan and the existing CI already cover it.)

- [ ] **Step 8: Commit**

```bash
git add crates/serrf-core/src/export.rs crates/serrf-core/src/lib.rs crates/serrf-cli/src/main.rs
git commit -m "Move CSV-export helpers into serrf-core as a shared writer-generic module"
```

---

### Task 2: Scaffold the `serrf-api` crate with a health endpoint

**Files:**
- Create: `crates/serrf-api/Cargo.toml`
- Create: `crates/serrf-api/src/main.rs`
- Create: `crates/serrf-api/src/app.rs`
- Create: `crates/serrf-api/tests/health.rs`
- Modify: `Cargo.toml` (workspace root — add `crates/serrf-api` to `members`)

**Interfaces:**
- Produces: `serrf_api::app::build_app() -> axum::Router` (the app's route table, built without binding a socket, so tests can mount it on an ephemeral port). `main.rs` binds it to `0.0.0.0:PORT` (env `PORT`, default `8080`).
- Consumes: nothing from `serrf-core` yet (that starts in Task 4).

- [ ] **Step 1: Write the failing integration test**

Create `crates/serrf-api/tests/health.rs`:

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

#[tokio::test]
async fn health_returns_200_ok() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/health")).send().await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
}
```

- [ ] **Step 2: Create `crates/serrf-api/Cargo.toml`**

```toml
[package]
name = "serrf-api"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "serrf-api"
path = "src/main.rs"

[lib]
name = "serrf_api"
path = "src/lib.rs"

[dependencies]
serrf-core = { path = "../serrf-core" }
axum = { version = "0.7", features = ["multipart"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
tempfile = "3"
async-stream = "0.3"
zip = "0.6"
thiserror = "1.0"

[dev-dependencies]
reqwest = { version = "0.12", features = ["json", "multipart", "stream"] }
```

- [ ] **Step 3: Create `crates/serrf-api/src/lib.rs`**

```rust
pub mod app;
```

- [ ] **Step 4: Create `crates/serrf-api/src/app.rs` with the health route**

```rust
pub fn build_app() -> axum::Router {
    axum::Router::new().route("/health", axum::routing::get(health))
}

async fn health() -> &'static str {
    "ok"
}
```

- [ ] **Step 5: Create `crates/serrf-api/src/main.rs`**

```rust
#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.unwrap();
    println!("serrf-api listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, serrf_api::app::build_app()).await.unwrap();
}
```

- [ ] **Step 6: Add `crates/serrf-api` to the workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/serrf-core", "crates/serrf-cli", "crates/serrf-api"]
```

- [ ] **Step 7: Run the test to verify it passes**

Run: `cargo test -p serrf-api`
Expected: PASS, 1 test.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/serrf-api
git commit -m "Scaffold serrf-api crate with a health endpoint"
```

---

### Task 3: `JobId` and `JobStore` — pure, HTTP-free job tracking

**Files:**
- Create: `crates/serrf-api/src/job.rs`
- Modify: `crates/serrf-api/src/lib.rs` (add `pub mod job;`)
- Modify: `crates/serrf-api/Cargo.toml` (add `ndarray` as a dev-dependency — the unit test below constructs a `serrf_core::PipelineOutput` by hand via `ndarray::Array2::zeros(...)`, and `ndarray` is currently only a dependency of `serrf-core`, not `serrf-api`, so naming it directly won't compile without this)

**Interfaces:**
- Produces:
  - `pub struct JobId(uuid::Uuid)` — `Copy + Clone + PartialEq + Eq + Hash + Display`; `JobId::new() -> Self`; `JobId::parse(s: &str) -> Result<Self, uuid::Error>`.
  - `#[derive(Clone, Debug, PartialEq, serde::Serialize)] #[serde(tag = "status", rename_all = "lowercase")] pub enum JobEvent { Queued, Progress { stage: String, current: usize, total: usize }, Completed, Failed { error: String } }`; `impl JobEvent { pub fn is_terminal(&self) -> bool }`.
  - `pub struct CompletedJob { pub compound_labels: Vec<String>, pub sample_type: Vec<Option<String>>, pub output: serrf_core::PipelineOutput }`.
  - `#[derive(Clone)] pub struct JobStore { ... }` with:
    - `JobStore::new() -> Self`
    - `fn create(&self) -> (JobId, tokio::sync::watch::Receiver<JobEvent>)` — inserts a `Queued` job, returns its id and a receiver subscribed to its events.
    - `fn push_progress(&self, id: JobId, event: JobEvent)` — no-op if the id is unknown (defensive; should not happen in practice).
    - `fn complete(&self, id: JobId, completed: CompletedJob)`
    - `fn fail(&self, id: JobId, error: String)`
    - `fn subscribe(&self, id: JobId) -> Option<tokio::sync::watch::Receiver<JobEvent>>`
    - `fn with_completed<R>(&self, id: JobId, f: impl FnOnce(&CompletedJob) -> R) -> Option<JobStoreLookup<R>>` where `JobStoreLookup<R>` is an enum `{ NotReady, Failed(String), Ready(R) }` and the outer `Option` is `None` for an unknown job id — lets callers build a response from the completed job data without cloning `PipelineOutput` or holding the lock across an `.await`.
- Consumes: `serrf_core::PipelineOutput` (Task 4 onward populate `CompletedJob`).

- [ ] **Step 1: Add `ndarray` as a dev-dependency in `crates/serrf-api/Cargo.toml`**

Add to the `[dev-dependencies]` section (alongside `reqwest`):

```toml
ndarray = "0.15"
```

- [ ] **Step 2: Write the failing unit tests**

Append to `crates/serrf-api/src/job.rs` (create the file with just this test module first):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_completed() -> CompletedJob {
        CompletedJob {
            compound_labels: vec!["c1".to_string()],
            sample_type: vec![Some("qc".to_string())],
            output: serrf_core::PipelineOutput {
                raw: ndarray::Array2::zeros((1, 1)),
                serrf: ndarray::Array2::zeros((1, 1)),
                qc_rsd_raw: vec![0.1],
                qc_rsd_serrf: vec![0.01],
                validate_rsd_raw: std::collections::HashMap::new(),
                validate_rsd_serrf: std::collections::HashMap::new(),
                sample_order: vec!["s1".to_string()],
            },
        }
    }

    #[test]
    fn a_new_job_starts_queued() {
        let store = JobStore::new();
        let (id, rx) = store.create();
        assert_eq!(*rx.borrow(), JobEvent::Queued);
        assert!(store.subscribe(id).is_some());
    }

    #[test]
    fn subscribe_returns_none_for_an_unknown_job() {
        let store = JobStore::new();
        assert!(store.subscribe(JobId::new()).is_none());
    }

    #[test]
    fn push_progress_updates_the_watched_event() {
        let store = JobStore::new();
        let (id, mut rx) = store.create();
        store.push_progress(id, JobEvent::Progress { stage: "SERRF normalization".into(), current: 3, total: 10 });
        assert!(rx.has_changed().unwrap());
        assert_eq!(
            *rx.borrow_and_update(),
            JobEvent::Progress { stage: "SERRF normalization".into(), current: 3, total: 10 }
        );
    }

    #[test]
    fn push_progress_on_an_unknown_job_is_a_silent_no_op() {
        let store = JobStore::new();
        store.push_progress(JobId::new(), JobEvent::Progress { stage: "x".into(), current: 1, total: 1 });
    }

    #[test]
    fn complete_sets_a_terminal_event_and_stores_the_result() {
        let store = JobStore::new();
        let (id, mut rx) = store.create();
        store.complete(id, sample_completed());
        assert!(rx.has_changed().unwrap());
        assert_eq!(*rx.borrow_and_update(), JobEvent::Completed);
        match store.with_completed(id, |c| c.compound_labels.clone()) {
            Some(JobStoreLookup::Ready(labels)) => assert_eq!(labels, vec!["c1".to_string()]),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn fail_sets_a_terminal_event_with_the_error_message() {
        let store = JobStore::new();
        let (id, mut rx) = store.create();
        store.fail(id, "boom".to_string());
        assert!(rx.has_changed().unwrap());
        assert_eq!(*rx.borrow_and_update(), JobEvent::Failed { error: "boom".to_string() });
        match store.with_completed(id, |c| c.compound_labels.clone()) {
            Some(JobStoreLookup::Failed(msg)) => assert_eq!(msg, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn with_completed_reports_not_ready_before_completion() {
        let store = JobStore::new();
        let (id, _rx) = store.create();
        match store.with_completed(id, |c| c.compound_labels.clone()) {
            Some(JobStoreLookup::NotReady) => {}
            other => panic!("expected NotReady, got {other:?}"),
        }
    }

    #[test]
    fn with_completed_reports_not_found_for_an_unknown_job() {
        let store = JobStore::new();
        assert!(store.with_completed(JobId::new(), |c| c.compound_labels.clone()).is_none());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p serrf-api --lib job::`
Expected: FAIL to compile — none of `JobId`, `JobEvent`, `CompletedJob`, `JobStore`, `JobStoreLookup` exist yet.

- [ ] **Step 4: Add `pub mod job;` to `crates/serrf-api/src/lib.rs`**

```rust
pub mod app;
pub mod job;
```

- [ ] **Step 5: Implement `crates/serrf-api/src/job.rs` above the test module**

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(uuid::Uuid);

impl JobId {
    pub fn new() -> Self {
        JobId(uuid::Uuid::new_v4())
    }

    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(JobId(uuid::Uuid::parse_str(s)?))
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum JobEvent {
    Queued,
    Progress { stage: String, current: usize, total: usize },
    Completed,
    Failed { error: String },
}

impl JobEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobEvent::Completed | JobEvent::Failed { .. })
    }
}

pub struct CompletedJob {
    pub compound_labels: Vec<String>,
    pub sample_type: Vec<Option<String>>,
    pub output: serrf_core::PipelineOutput,
}

enum JobResult {
    Pending,
    Done(CompletedJob),
    Errored(String),
}

struct JobHandle {
    events: tokio::sync::watch::Sender<JobEvent>,
    result: JobResult,
}

#[derive(Debug)]
pub enum JobStoreLookup<R> {
    NotReady,
    Failed(String),
    Ready(R),
}

#[derive(Clone)]
pub struct JobStore {
    jobs: Arc<RwLock<HashMap<JobId, JobHandle>>>,
}

impl JobStore {
    pub fn new() -> Self {
        JobStore { jobs: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn create(&self) -> (JobId, tokio::sync::watch::Receiver<JobEvent>) {
        let id = JobId::new();
        let (tx, rx) = tokio::sync::watch::channel(JobEvent::Queued);
        self.jobs.write().unwrap().insert(id, JobHandle { events: tx, result: JobResult::Pending });
        (id, rx)
    }

    pub fn push_progress(&self, id: JobId, event: JobEvent) {
        if let Some(handle) = self.jobs.read().unwrap().get(&id) {
            let _ = handle.events.send(event);
        }
    }

    pub fn complete(&self, id: JobId, completed: CompletedJob) {
        if let Some(handle) = self.jobs.write().unwrap().get_mut(&id) {
            handle.result = JobResult::Done(completed);
            let _ = handle.events.send(JobEvent::Completed);
        }
    }

    pub fn fail(&self, id: JobId, error: String) {
        if let Some(handle) = self.jobs.write().unwrap().get_mut(&id) {
            let _ = handle.events.send(JobEvent::Failed { error: error.clone() });
            handle.result = JobResult::Errored(error);
        }
    }

    pub fn subscribe(&self, id: JobId) -> Option<tokio::sync::watch::Receiver<JobEvent>> {
        self.jobs.read().unwrap().get(&id).map(|h| h.events.subscribe())
    }

    pub fn with_completed<R>(&self, id: JobId, f: impl FnOnce(&CompletedJob) -> R) -> Option<JobStoreLookup<R>> {
        let jobs = self.jobs.read().unwrap();
        let handle = jobs.get(&id)?;
        Some(match &handle.result {
            JobResult::Pending => JobStoreLookup::NotReady,
            JobResult::Errored(e) => JobStoreLookup::Failed(e.clone()),
            JobResult::Done(completed) => JobStoreLookup::Ready(f(completed)),
        })
    }
}

impl Default for JobStore {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p serrf-api --lib job::`
Expected: PASS, 8 tests.

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-api/src/job.rs crates/serrf-api/src/lib.rs crates/serrf-api/Cargo.toml
git commit -m "Add JobId/JobEvent/JobStore — pure in-memory job tracking"
```

---

### Task 4: `ApiError` and the upload endpoint (`POST /api/jobs`)

**Files:**
- Create: `crates/serrf-api/src/error.rs`
- Create: `crates/serrf-api/src/routes/mod.rs`
- Create: `crates/serrf-api/src/routes/upload.rs`
- Create: `crates/serrf-api/tests/upload.rs`
- Modify: `crates/serrf-api/src/lib.rs` (add `pub mod error;` and `pub mod routes;`)
- Modify: `crates/serrf-api/src/app.rs` (wire `JobStore` into shared state, add the route)

**Interfaces:**
- Produces: `pub enum ApiError { BadRequest(String), NotFound, Internal(String) }` implementing `axum::response::IntoResponse` (JSON body `{"error": "..."}`, status 400/404/500 respectively). `pub struct AppState { pub jobs: crate::job::JobStore }` (`Clone`). `app::build_app() -> Router` now takes no state param but constructs a fresh `JobStore` internally (tests get a fresh store per `spawn_app()` call, matching Task 2's pattern).
- Consumes: `serrf_core::parse::read_data(path: &Path) -> Result<Dataset, SerrfError>`, `serrf_core::validate::validate(dataset: &Dataset) -> Result<ValidatedSamples, SerrfError>`, `serrf_core::normalize(&Dataset, &ValidatedSamples, &SerrfConfig, impl FnMut(Progress) + Send) -> Result<PipelineOutput, SerrfError>`, `crate::job::{JobStore, JobId, JobEvent, CompletedJob}` (Task 3).

- [ ] **Step 1: Write the failing integration test**

Create `crates/serrf-api/tests/upload.rs`. It needs a small, valid transposed-layout CSV fixture (2 batches, 6 QC + 4 sample each, matching `serrf-core`'s own `validate.rs`/`pipeline.rs` test fixtures' shape so it passes validation and normalizes in well under a second):

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
    // header row: No, label, then 20 sample columns (12 QC across batch A/B, 8 samples across A/B)
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

#[tokio::test]
async fn uploading_a_valid_csv_returns_202_with_a_job_id() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text(valid_csv_fixture())
        .file_name("dataset.csv")
        .mime_str("text/csv")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();

    assert_eq!(response.status(), 202);
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["job_id"].is_string());
}

#[tokio::test]
async fn uploading_an_unparseable_file_returns_400_with_a_structured_error() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text("not,a,valid,serrf,layout\n1,2,3,4")
        .file_name("dataset.csv")
        .mime_str("text/csv")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();

    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn uploading_a_file_with_an_unsupported_extension_returns_400() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text("whatever").file_name("dataset.txt").mime_str("text/plain").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();

    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn a_completed_job_is_reachable_via_the_returned_job_id() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text(valid_csv_fixture()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    // Poll until the job finishes (it's small — should be near-instant).
    for _ in 0..100 {
        let status = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
        if status.status() != 425 {
            assert_eq!(status.status(), 200, "expected the small job to complete successfully");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job never completed within the polling window");
}
```

(The last test references `GET /api/jobs/:id/result` returning `200`/`425`, which Task 6 implements — for now it will 404, since only the upload route exists. That's expected: this step's goal is the first three tests; leave the fourth in the file now since it documents the target behavior, but only require the first three to pass at Step 5 of *this* task. Mark it `#[ignore]` for now and remove the `#[ignore]` in Task 6.)

Add `#[ignore = "requires GET /api/jobs/:id/result from Task 6"]` above `a_completed_job_is_reachable_via_the_returned_job_id`.

- [ ] **Step 2: Run the first three tests to verify they fail**

Run: `cargo test -p serrf-api --test upload`
Expected: FAIL — `POST /api/jobs` doesn't exist yet (404s, so status assertions fail).

- [ ] **Step 3: Create `crates/serrf-api/src/error.rs`**

```rust
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::http::StatusCode;

pub enum ApiError {
    BadRequest(String),
    NotFound,
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "job not found".to_string()),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<serrf_core::error::SerrfError> for ApiError {
    fn from(e: serrf_core::error::SerrfError) -> Self {
        ApiError::BadRequest(e.to_string())
    }
}
```

- [ ] **Step 4: Create `crates/serrf-api/src/routes/mod.rs`**

```rust
pub mod upload;
```

- [ ] **Step 5: Create `crates/serrf-api/src/routes/upload.rs`**

```rust
use crate::app::AppState;
use crate::error::ApiError;
use crate::job::JobEvent;
use axum::extract::{Multipart, State};
use axum::Json;

pub async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let mut file_name: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| ApiError::BadRequest(e.to_string()))? {
        if field.name() == Some("file") {
            file_name = field.file_name().map(|s| s.to_string());
            bytes = Some(field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?.to_vec());
        }
    }

    let file_name = file_name.ok_or_else(|| ApiError::BadRequest("missing 'file' field".to_string()))?;
    let bytes = bytes.ok_or_else(|| ApiError::BadRequest("missing 'file' field".to_string()))?;

    let extension = std::path::Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| ApiError::BadRequest("uploaded file has no extension".to_string()))?;
    if extension != "csv" && extension != "xlsx" {
        return Err(ApiError::BadRequest(format!("unsupported file extension: {extension}")));
    }

    let mut temp_file = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    std::io::Write::write_all(&mut temp_file, &bytes).map_err(|e| ApiError::Internal(e.to_string()))?;

    let dataset = serrf_core::parse::read_data(temp_file.path())?;
    let samples = serrf_core::validate::validate(&dataset)?;

    let (job_id, _rx) = state.jobs.create();
    let jobs = state.jobs.clone();
    let compound_labels = dataset.compounds.label.clone();
    let sample_type = samples.sample_type.clone();

    tokio::task::spawn_blocking(move || {
        let progress_jobs = jobs.clone();
        let result = serrf_core::normalize(&dataset, &samples, &serrf_core::SerrfConfig::default(), move |p| {
            progress_jobs.push_progress(job_id, JobEvent::Progress { stage: p.stage, current: p.current, total: p.total });
        });
        match result {
            Ok(output) => jobs.complete(job_id, crate::job::CompletedJob { compound_labels, sample_type, output }),
            Err(e) => jobs.fail(job_id, e.to_string()),
        }
    });

    Ok((axum::http::StatusCode::ACCEPTED, Json(serde_json::json!({ "job_id": job_id.to_string() }))))
}
```

- [ ] **Step 6: Wire `AppState` and the route into `crates/serrf-api/src/app.rs`**

```rust
#[derive(Clone)]
pub struct AppState {
    pub jobs: crate::job::JobStore,
}

pub fn build_app() -> axum::Router {
    let state = AppState { jobs: crate::job::JobStore::new() };
    axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
```

- [ ] **Step 7: Add `pub mod error;` and `pub mod routes;` to `crates/serrf-api/src/lib.rs`**

```rust
pub mod app;
pub mod error;
pub mod job;
pub mod routes;
```

- [ ] **Step 8: Run the three non-ignored tests to verify they pass**

Run: `cargo test -p serrf-api --test upload`
Expected: PASS, 3 tests (1 ignored).

- [ ] **Step 9: Commit**

```bash
git add crates/serrf-api
git commit -m "Add ApiError and POST /api/jobs upload endpoint"
```

---

### Task 5: SSE progress endpoint (`GET /api/jobs/:id/events`)

**Files:**
- Create: `crates/serrf-api/src/routes/events.rs`
- Create: `crates/serrf-api/tests/events.rs`
- Modify: `crates/serrf-api/src/routes/mod.rs`
- Modify: `crates/serrf-api/src/app.rs`

**Interfaces:**
- Produces: `GET /api/jobs/:id/events` — SSE stream of `JobEvent`s (as JSON `data:` payloads, `event:` name mirrors the JSON `status` tag), closing after the first terminal event; `404` for an unknown job id (returned as a normal JSON error response before the SSE stream starts, since the id is validated up front).
- Consumes: `crate::job::{JobStore, JobEvent}` (Task 3), `crate::error::ApiError` (Task 4).

- [ ] **Step 1: Write the failing integration test**

Create `crates/serrf-api/tests/events.rs` (reuses the `spawn_app`/`valid_csv_fixture` helpers — duplicate them here, matching the pattern already used in `upload.rs`, since each integration test file compiles as its own crate):

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

async fn upload_fixture(base_url: &str, client: &reqwest::Client) -> String {
    let part = reqwest::multipart::Part::text(valid_csv_fixture()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    body["job_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn events_stream_ends_with_a_terminal_event() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let job_id = upload_fixture(&base_url, &client).await;

    let response = client.get(format!("{base_url}/api/jobs/{job_id}/events")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();

    assert!(body.contains("event: completed") || body.contains("event: failed"), "expected a terminal SSE event, got: {body}");
}

#[tokio::test]
async fn events_for_an_unknown_job_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let fake_id = uuid::Uuid::new_v4();

    let response = client.get(format!("{base_url}/api/jobs/{fake_id}/events")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn events_for_a_malformed_job_id_returns_400() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/api/jobs/not-a-uuid/events")).send().await.unwrap();

    assert_eq!(response.status(), 400);
}
```

Add `uuid = { version = "1", features = ["v4"] }` to `crates/serrf-api/Cargo.toml`'s `[dev-dependencies]` (needed for `events_for_an_unknown_job_returns_404`'s fake id).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p serrf-api --test events`
Expected: FAIL — route doesn't exist, all requests 404 (so the "malformed id" 400 test also fails).

- [ ] **Step 3: Implement `crates/serrf-api/src/routes/events.rs`**

```rust
use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobEvent, JobId};
use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use futures_core::Stream;

pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;
    let mut rx = state.jobs.subscribe(job_id).ok_or(ApiError::NotFound)?;

    let stream = async_stream::stream! {
        loop {
            let event = rx.borrow_and_update().clone();
            yield Ok(to_sse_event(&event));
            if event.is_terminal() {
                break;
            }
            if rx.changed().await.is_err() {
                break;
            }
        }
    };

    Ok(Sse::new(stream))
}

fn to_sse_event(event: &JobEvent) -> Event {
    let name = match event {
        JobEvent::Queued => "queued",
        JobEvent::Progress { .. } => "progress",
        JobEvent::Completed => "completed",
        JobEvent::Failed { .. } => "failed",
    };
    Event::default().event(name).json_data(event).unwrap()
}
```

Add `futures-core = "0.3"` to `crates/serrf-api/Cargo.toml`'s `[dependencies]` (the `Stream` trait bound above).

- [ ] **Step 4: Wire the route into `crates/serrf-api/src/app.rs`**

Replace the `build_app` function with (adds one `.route(...)` chain call to Task 4's version, everything else unchanged):

```rust
pub fn build_app() -> axum::Router {
    let state = AppState { jobs: crate::job::JobStore::new() };
    axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .with_state(state)
}
```

- [ ] **Step 5: Add `pub mod events;` to `crates/serrf-api/src/routes/mod.rs`**

```rust
pub mod events;
pub mod upload;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p serrf-api --test events`
Expected: PASS, 3 tests.

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-api
git commit -m "Add SSE progress endpoint GET /api/jobs/:id/events"
```

---

### Task 6: Result endpoint (`GET /api/jobs/:id/result`)

**Files:**
- Create: `crates/serrf-api/src/routes/result.rs`
- Modify: `crates/serrf-api/src/routes/mod.rs`
- Modify: `crates/serrf-api/src/app.rs`
- Modify: `crates/serrf-api/tests/upload.rs` (remove the `#[ignore]` from Task 4's fourth test)

**Interfaces:**
- Produces: `GET /api/jobs/:id/result` — `200` JSON `{ "qc_rsd_raw": [f64], "qc_rsd_serrf": [f64], "validate_rsd_raw": {String: [f64]}, "validate_rsd_serrf": {String: [f64]}, "compound_labels": [String], "pca_before": {"pc1": [f64], "pc2": [f64]}, "pca_after": {"pc1": [f64], "pc2": [f64]} }` once the job is `Completed`; `425` (Too Early) with `{"status": "..."}` while still running; `404` unknown job id; `400` malformed id; `500` with the stored error message if the job `Failed`.
- Consumes: `crate::job::{JobStore, JobStoreLookup}` (Task 3), `serrf_core::export::{std_dev, filter_rows_with_variance}` (Task 1), `serrf_core::pca::pca_first_two` (Plan 1).

- [ ] **Step 1: Remove `#[ignore]` from `a_completed_job_is_reachable_via_the_returned_job_id` in `crates/serrf-api/tests/upload.rs`**

- [ ] **Step 2: Write the additional failing integration test**

Add to `crates/serrf-api/tests/upload.rs` (this test targets not-ready/not-found cases the fourth Task-4 test doesn't cover):

```rust
#[tokio::test]
async fn result_for_an_unknown_job_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let fake_id = uuid::Uuid::new_v4();

    let response = client.get(format!("{base_url}/api/jobs/{fake_id}/result")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p serrf-api --test upload`
Expected: FAIL — `GET /api/jobs/:id/result` route doesn't exist (404 for everything, so the new test also fails since it currently 404s for the wrong reason but let's be precise: it will actually already pass by coincidence since no route exists yet — rerun after Step 2 confirms `a_completed_job_is_reachable_via_the_returned_job_id` fails with a connection/404 mismatch). Confirm specifically that `a_completed_job_is_reachable_via_the_returned_job_id` fails.

- [ ] **Step 4: Implement `crates/serrf-api/src/routes/result.rs`**

```rust
use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobId, JobStoreLookup};
use axum::extract::{Path, State};
use axum::Json;

#[derive(serde::Serialize)]
struct PcaJson {
    pc1: Vec<f64>,
    pc2: Vec<f64>,
}

#[derive(serde::Serialize)]
struct ResultJson {
    compound_labels: Vec<String>,
    qc_rsd_raw: Vec<f64>,
    qc_rsd_serrf: Vec<f64>,
    validate_rsd_raw: std::collections::HashMap<String, Vec<f64>>,
    validate_rsd_serrf: std::collections::HashMap<String, Vec<f64>>,
    pca_before: PcaJson,
    pca_after: PcaJson,
}

pub async fn result(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<ResultJson>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;

    let lookup = state
        .jobs
        .with_completed(job_id, |completed| {
            let sds_before: Vec<f64> = (0..completed.output.raw.nrows())
                .map(|i| serrf_core::export::std_dev(&completed.output.raw.row(i).to_vec()))
                .collect();
            let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.raw, &sds_before));
            let sds_after: Vec<f64> = (0..completed.output.serrf.nrows())
                .map(|i| serrf_core::export::std_dev(&completed.output.serrf.row(i).to_vec()))
                .collect();
            let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.serrf, &sds_after));

            ResultJson {
                compound_labels: completed.compound_labels.clone(),
                qc_rsd_raw: completed.output.qc_rsd_raw.clone(),
                qc_rsd_serrf: completed.output.qc_rsd_serrf.clone(),
                validate_rsd_raw: completed.output.validate_rsd_raw.clone(),
                validate_rsd_serrf: completed.output.validate_rsd_serrf.clone(),
                pca_before: PcaJson { pc1: pca_before.pc1, pc2: pca_before.pc2 },
                pca_after: PcaJson { pc1: pca_after.pc1, pc2: pca_after.pc2 },
            }
        })
        .ok_or(ApiError::NotFound)?;

    match lookup {
        JobStoreLookup::Ready(json) => Ok(Json(json)),
        JobStoreLookup::NotReady => Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => Err(ApiError::Internal(msg)),
    }
}
```

- [ ] **Step 5: Add the `NotReady` variant to `ApiError` in `crates/serrf-api/src/error.rs`**

```rust
pub enum ApiError {
    BadRequest(String),
    NotFound,
    NotReady,
    Internal(String),
}
```

And in `IntoResponse`'s match:

```rust
ApiError::NotReady => (StatusCode::TOO_EARLY, "job is still running".to_string()),
```

(`StatusCode::TOO_EARLY` is 425, from the `http` crate re-exported by `axum::http`.)

- [ ] **Step 6: Wire the route into `crates/serrf-api/src/app.rs`**

Replace the `build_app` function with (adds one `.route(...)` chain call to Task 5's version, everything else unchanged):

```rust
pub fn build_app() -> axum::Router {
    let state = AppState { jobs: crate::job::JobStore::new() };
    axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .route("/api/jobs/:id/result", axum::routing::get(crate::routes::result::result))
        .with_state(state)
}
```

- [ ] **Step 7: Add `pub mod result;` to `crates/serrf-api/src/routes/mod.rs`**

```rust
pub mod events;
pub mod result;
pub mod upload;
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p serrf-api --test upload`
Expected: PASS, 5 tests.

- [ ] **Step 9: Commit**

```bash
git add crates/serrf-api
git commit -m "Add GET /api/jobs/:id/result endpoint"
```

---

### Task 7: Download endpoint (`GET /api/jobs/:id/download`) — zip of CSVs + PNG report

**Files:**
- Create: `crates/serrf-api/src/routes/download.rs`
- Create: `crates/serrf-api/tests/download.rs`
- Modify: `crates/serrf-api/src/routes/mod.rs`
- Modify: `crates/serrf-api/src/app.rs`

**Interfaces:**
- Produces: `GET /api/jobs/:id/download` — `200` with `Content-Type: application/zip`, `Content-Disposition: attachment; filename="serrf-results.zip"`, body containing `normalized-imputed.csv`, `normalized-serrf.csv`, `qc-rsds.csv`, `report.png`; same `404`/`400`/`425`/`500` semantics as Task 6 for a not-found/malformed/not-ready/failed job.
- Consumes: `serrf_core::export::{write_matrix_csv, write_rsd_csv}` (Task 1), `serrf_core::report::render_report` (Plan 1), `crate::job::JobStoreLookup` (Task 3).

- [ ] **Step 1: Write the failing integration test**

Create `crates/serrf-api/tests/download.rs` (duplicate the `spawn_app`/`valid_csv_fixture`/`upload_fixture` helpers from `events.rs`, same pattern):

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
    let part = reqwest::multipart::Part::text(valid_csv_fixture()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    for _ in 0..100 {
        let status = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
        if status.status() == 200 {
            return job_id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job never completed within the polling window");
}

#[tokio::test]
async fn download_returns_a_zip_with_the_expected_entries() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let job_id = upload_and_wait_for_completion(&base_url, &client).await;

    let response = client.get(format!("{base_url}/api/jobs/{job_id}/download")).send().await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/zip");
    let bytes = response.bytes().await.unwrap();
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).unwrap();
    let mut names: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_string()).collect();
    names.sort();
    assert_eq!(names, vec!["normalized-imputed.csv", "normalized-serrf.csv", "qc-rsds.csv", "report.png"]);
}

#[tokio::test]
async fn download_for_an_unknown_job_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let fake_id = uuid::Uuid::new_v4();

    let response = client.get(format!("{base_url}/api/jobs/{fake_id}/download")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p serrf-api --test download`
Expected: FAIL — route doesn't exist.

- [ ] **Step 3: Implement `crates/serrf-api/src/routes/download.rs`**

```rust
use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobId, JobStoreLookup};
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;

pub async fn download(State(state): State<AppState>, Path(id): Path<String>) -> Result<impl IntoResponse, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;

    let lookup = state
        .jobs
        .with_completed(job_id, |completed| build_zip(completed))
        .ok_or(ApiError::NotFound)?;

    let zip_bytes = match lookup {
        JobStoreLookup::Ready(bytes) => bytes.map_err(ApiError::Internal)?,
        JobStoreLookup::NotReady => return Err(ApiError::NotReady),
        JobStoreLookup::Failed(msg) => return Err(ApiError::Internal(msg)),
    };

    Ok((
        [(header::CONTENT_TYPE, "application/zip"), (header::CONTENT_DISPOSITION, "attachment; filename=\"serrf-results.zip\"")],
        zip_bytes,
    ))
}

fn build_zip(completed: &crate::job::CompletedJob) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    let options = zip::write::FileOptions::default();

    zip.start_file("normalized-imputed.csv", options).map_err(|e| e.to_string())?;
    serrf_core::export::write_matrix_csv(&mut zip, &completed.output.sample_order, &completed.compound_labels, &completed.output.raw).map_err(|e| e.to_string())?;

    zip.start_file("normalized-serrf.csv", options).map_err(|e| e.to_string())?;
    serrf_core::export::write_matrix_csv(&mut zip, &completed.output.sample_order, &completed.compound_labels, &completed.output.serrf).map_err(|e| e.to_string())?;

    zip.start_file("qc-rsds.csv", options).map_err(|e| e.to_string())?;
    serrf_core::export::write_rsd_csv(
        &mut zip,
        &completed.compound_labels,
        &completed.output.qc_rsd_raw,
        &completed.output.qc_rsd_serrf,
        &completed.output.validate_rsd_raw,
        &completed.output.validate_rsd_serrf,
    )
    .map_err(|e| e.to_string())?;

    let sds_before: Vec<f64> = (0..completed.output.raw.nrows()).map(|i| serrf_core::export::std_dev(&completed.output.raw.row(i).to_vec())).collect();
    let pca_before = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.raw, &sds_before));
    let sds_after: Vec<f64> = (0..completed.output.serrf.nrows()).map(|i| serrf_core::export::std_dev(&completed.output.serrf.row(i).to_vec())).collect();
    let pca_after = serrf_core::pca::pca_first_two(&serrf_core::export::filter_rows_with_variance(&completed.output.serrf, &sds_after));

    let png_file = tempfile::Builder::new().suffix(".png").tempfile().map_err(|e| e.to_string())?;
    serrf_core::report::render_report(
        png_file.path(),
        &completed.output.qc_rsd_raw,
        &completed.output.qc_rsd_serrf,
        &pca_before,
        &pca_after,
        &completed.sample_type,
    )
    .map_err(|e| e.to_string())?;
    let png_bytes = std::fs::read(png_file.path()).map_err(|e| e.to_string())?;
    zip.start_file("report.png", options).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut zip, &png_bytes).map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}
```

- [ ] **Step 4: Wire the route into `crates/serrf-api/src/app.rs`**

Replace the `build_app` function with (adds one `.route(...)` chain call to Task 6's version, everything else unchanged):

```rust
pub fn build_app() -> axum::Router {
    let state = AppState { jobs: crate::job::JobStore::new() };
    axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .route("/api/jobs/:id/result", axum::routing::get(crate::routes::result::result))
        .route("/api/jobs/:id/download", axum::routing::get(crate::routes::download::download))
        .with_state(state)
}
```

- [ ] **Step 5: Add `pub mod download;` to `crates/serrf-api/src/routes/mod.rs`**

```rust
pub mod download;
pub mod events;
pub mod result;
pub mod upload;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p serrf-api --test download`
Expected: PASS, 2 tests.

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-api
git commit -m "Add GET /api/jobs/:id/download endpoint (zip of CSVs + PNG report)"
```

---

### Task 8: Failure-path integration test — a compound that can't normalize doesn't crash the job

**Files:**
- Create: `crates/serrf-api/tests/failure_path.rs`

**Interfaces:**
- Consumes: everything from Tasks 4-7. No new production code — this task is pure test coverage for a path Plan 1 already made safe at the `serrf-core` level (the C1 non-finite-value fixes), verified here at the HTTP boundary.

- [ ] **Step 1: Write the test**

`serrf-core`'s `normalize()` already never panics on pathological per-compound data (Plan 1's C1 fixes) — it returns `NaN` rows instead. This test confirms that behavior survives the trip through `serrf-api`'s job pipeline instead of being silently swallowed or turned into a `Failed` job when it shouldn't be.

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

fn csv_with_one_all_missing_compound() -> String {
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
    // Compound0 is normal; Compound1 is entirely missing (blank cells) in every sample.
    let mut normal_row = vec!["1".to_string(), "Compound0".to_string()];
    let mut missing_row = vec!["2".to_string(), "Compound1".to_string()];
    for j in 0..20 {
        normal_row.push((100.0 + j as f64 % 3.0).to_string());
        missing_row.push(String::new());
    }
    lines.push(normal_row.join(","));
    lines.push(missing_row.join(","));
    lines.join("\n")
}

#[tokio::test]
async fn a_job_with_one_unnormalizable_compound_still_completes_successfully() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text(csv_with_one_all_missing_compound()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    for _ in 0..100 {
        let status = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
        if status.status() == 200 {
            let result: serde_json::Value = status.json().await.unwrap();
            assert_eq!(result["compound_labels"].as_array().unwrap().len(), 2);
            // Compound1 (index 1) comes back as NaN, not a job failure.
            assert!(result["qc_rsd_serrf"][1].as_f64().is_none(), "expected NaN (non-numeric in JSON) for the unnormalizable compound");
            assert!(result["qc_rsd_serrf"][0].as_f64().is_some(), "expected Compound0 to normalize normally");
            return;
        }
        assert_ne!(status.status(), 500, "job should not fail outright because of one bad compound");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job never completed within the polling window");
}
```

(`serde_json` serializes `f64::NAN` as JSON `null` by default — `as_f64()` on a `null` value returns `None`, which is exactly the assertion above.)

- [ ] **Step 2: Run the test to verify it fails for the right reason initially, then run again after nothing changes**

Run: `cargo test -p serrf-api --test failure_path`
Expected: PASS immediately — no new production code is needed for this task; it's a regression test proving Tasks 4-7's wiring correctly surfaces `serrf-core`'s existing panic-safety guarantees. If it fails, that's a real bug in the Task 4-7 wiring (e.g. accidentally propagating a per-compound `NaN` result as a job-level `Failed`) — fix the wiring, not the test.

- [ ] **Step 3: Commit**

```bash
git add crates/serrf-api/tests/failure_path.rs
git commit -m "Add integration test for the unnormalizable-compound-doesn't-fail-the-job path"
```

---

### Task 9: Slow end-to-end integration test against the real bundled dataset

**Files:**
- Create: `crates/serrf-api/tests/golden_e2e.rs`
- Modify: `crates/serrf-api/Cargo.toml` (dev-dependency on `tokio` with `time` feature already present via `["rt-multi-thread", ...]`; no change needed beyond what Task 2 added)

**Interfaces:**
- Consumes: everything built in Tasks 1-7, plus the bundled `golden/example-dataset.xlsx` fixture (already checked in from Plan 1).

- [ ] **Step 1: Write the test**

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

#[tokio::test]
async fn the_full_job_lifecycle_completes_for_the_real_bundled_dataset() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let bytes = std::fs::read("../../golden/example-dataset.xlsx").unwrap();
    let part = reqwest::multipart::Part::bytes(bytes).file_name("example-dataset.xlsx").mime_str("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let upload_response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    assert_eq!(upload_response.status(), 202);
    let body: serde_json::Value = upload_response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    // SSE stream should eventually reach a terminal event; the real dataset takes minutes.
    let events_response = client.get(format!("{base_url}/api/jobs/{job_id}/events")).timeout(std::time::Duration::from_secs(600)).send().await.unwrap();
    assert_eq!(events_response.status(), 200);
    let events_body = events_response.text().await.unwrap();
    assert!(events_body.contains("event: completed"), "expected the real dataset job to complete: {events_body}");

    let result_response = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
    assert_eq!(result_response.status(), 200);
    let result: serde_json::Value = result_response.json().await.unwrap();
    assert_eq!(result["compound_labels"].as_array().unwrap().len(), 268);

    let download_response = client.get(format!("{base_url}/api/jobs/{job_id}/download")).send().await.unwrap();
    assert_eq!(download_response.status(), 200);
    let zip_bytes = download_response.bytes().await.unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    assert_eq!(archive.len(), 4);
}
```

- [ ] **Step 2: Run the test to verify it passes**

Run: `cargo test -p serrf-api --test golden_e2e -- --nocapture`
Expected: PASS (slow — several minutes, matching `serrf-core`'s own golden test runtime).

- [ ] **Step 3: Commit**

```bash
git add crates/serrf-api/tests/golden_e2e.rs
git commit -m "Add slow end-to-end integration test against the real bundled dataset"
```

---

### Task 10: CI, coverage check, and final full-suite verification

**Files:**
- Modify: `.github/workflows/ci.yml` (no change expected — `cargo test --workspace` already picks up the new crate automatically; this task is verification, not new config)

- [ ] **Step 1: Run the full fast suite**

Run: `cargo test --workspace --exclude serrf-api -- --skip golden` — actually simpler: run everything except the two known-slow tests by name.

Run: `cargo test -p serrf-core --lib && cargo test -p serrf-cli --lib && cargo test -p serrf-api --lib --test health --test upload --test events --test download --test failure_path`
Expected: PASS, fast (well under a minute total).

- [ ] **Step 2: Run clippy and fmt across the workspace**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean, no warnings (matches the `rustfmt.toml`/CI setup from Plan 1).

- [ ] **Step 3: Run coverage**

Run: `cargo tarpaulin --workspace --exclude-files 'crates/serrf-cli/*' --ignore-tests -- --skip golden_e2e --skip golden_normalize`
Expected: ≥80% combined line coverage for `serrf-api` (per Global Constraints). If short, identify the gap and add a targeted unit/integration test — do not lower the bar.

- [ ] **Step 4: Run the two slow tests once to confirm the whole system works end-to-end**

Run: `cargo test -p serrf-core --test golden_normalize && cargo test -p serrf-cli --test cli && cargo test -p serrf-api --test golden_e2e`
Expected: PASS (this is the full ~15-20 minute slow-test run — same tests CI already runs on every push).

- [ ] **Step 5: Commit if Step 3 required new tests; otherwise this task produces no diff**

```bash
git add -A
git commit -m "Verify serrf-api coverage and full-suite green before merge" --allow-empty
```
