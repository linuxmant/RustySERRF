# Windows Standalone Executable Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a single `serrf-api-windows.exe` that embeds the Next.js frontend as static assets, so a non-technical Windows user can double-click it and get the running app in their browser — no Docker, Rust, or Node on their machine.

**Architecture:** `serrf-api` gains a default-off Cargo feature, `bundled-frontend`, that embeds `crates/serrf-api/static-dist/` via `rust-embed` and adds one axum fallback route serving those bytes; a local shell script (no GitHub Actions) builds the frontend as a static export, drops it into `static-dist/`, cross-compiles via `cross`/Docker for `x86_64-pc-windows-gnu`, then restores the committed placeholder.

**Tech Stack:** `rust-embed` (compile-time asset embedding), `open` (cross-platform "launch default browser"), `cross` + Docker (Windows cross-compilation, no local mingw/rustup-target setup), Next.js static export (`output: "export"`).

**Spec:** `docs/superpowers/specs/2026-09-01-windows-standalone-executable-design.md`

## Global Constraints

- TDD non-negotiable: write the failing test before implementation code, for every task below.
- No mocks/stubs/fakes — the axum integration test in Task 2 makes real HTTP requests against a real running instance (`reqwest`), same pattern as `tests/health.rs`/`tests/upload.rs`.
- The `bundled-frontend` feature must be default-off: plain `cargo build -p serrf-api` / `cargo test -p serrf-api` / `cargo clippy` (no `--features`) must behave exactly as they do today. Existing CI and `dev-start.sh` are not allowed to change.
- `crates/serrf-api/static-dist/` stays a **committed, tracked** directory (a small placeholder), never gitignored — `rust-embed` needs real content at compile time on a fresh clone, with or without the feature enabled.
- No new GitHub Actions workflow — the Windows build is a manual, repeatable local script.
- Known risk carried into Task 4: `serrf-core`'s `plotters` dependency pulls in `font-kit` transitively (confirmed via `grep -A3 'name = "plotters"' Cargo.lock` / `grep -A2 'name = "font-kit"' Cargo.lock`), which uses per-target-OS font backends (`dwrote`/DirectWrite when the target is Windows, not the Linux `freetype`/`fontconfig` path). This is expected to cross-compile cleanly under `cross`'s prebuilt Windows-gnu image, but Task 4 is the first real proof — if `cross build` fails on a `font-kit`/`dwrote` link error, stop and report it rather than guessing at a fix blind.

---

### Task 1: `static_assets` module — embedded-asset lookup, TDD against a test fixture

**Files:**
- Modify: `crates/serrf-api/Cargo.toml` (add optional `rust-embed` dependency + `bundled-frontend` feature)
- Create: `crates/serrf-api/static-dist/index.html` (committed placeholder — embedded by the real, non-test `Assets` type)
- Create: `crates/serrf-api/tests/fixtures/static/index.html` (test fixture)
- Create: `crates/serrf-api/tests/fixtures/static/_next/static/x.js` (test fixture, nested path)
- Create: `crates/serrf-api/src/static_assets.rs`
- Modify: `crates/serrf-api/src/lib.rs` (add the feature-gated module declaration)

**Interfaces:**
- Produces: `#[cfg(feature = "bundled-frontend")] pub fn serrf_api::static_assets::lookup(path: &str) -> Option<(std::borrow::Cow<'static, [u8]>, &'static str)>` — used by Task 2's fallback route.
- Consumes: nothing from earlier tasks (this is the first task).

- [ ] **Step 1: Add the optional dependency and feature**

Run:
```bash
cargo add rust-embed --optional -p serrf-api
```

Then add a `[features]` section to `crates/serrf-api/Cargo.toml` (create it if `cargo add` didn't; place it after `[dependencies]`):

```toml
[features]
bundled-frontend = ["dep:rust-embed"]
```

- [ ] **Step 2: Create the committed placeholder `static-dist/`**

Create `crates/serrf-api/static-dist/index.html`:

```html
<!doctype html>
<html>
  <head><meta charset="utf-8"><title>SERRF</title></head>
  <body>
    <h1>SERRF (dev placeholder)</h1>
    <p>Run <code>scripts/build-windows-release.sh</code> to embed the real frontend build.</p>
  </body>
</html>
```

- [ ] **Step 3: Create the test fixtures**

Create `crates/serrf-api/tests/fixtures/static/index.html`:

```html
<!doctype html>
<html><body><h1>Hello SERRF fixture</h1></body></html>
```

Create `crates/serrf-api/tests/fixtures/static/_next/static/x.js`:

```js
console.log("fixture asset");
```

- [ ] **Step 4: Write the failing tests**

Create `crates/serrf-api/src/static_assets.rs` with just this (no implementation yet — it won't compile, which is the expected failing state):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_embed::RustEmbed;

    #[derive(RustEmbed)]
    #[folder = "tests/fixtures/static/"]
    struct TestAssets;

    #[test]
    fn empty_path_resolves_to_index_html() {
        let (bytes, mime) = lookup_in::<TestAssets>("").unwrap();
        assert_eq!(mime, "text/html");
        assert!(String::from_utf8(bytes.into_owned()).unwrap().contains("Hello SERRF fixture"));
    }

    #[test]
    fn index_html_path_resolves_directly() {
        let (_, mime) = lookup_in::<TestAssets>("index.html").unwrap();
        assert_eq!(mime, "text/html");
    }

    #[test]
    fn nested_asset_resolves_with_javascript_mime() {
        let (bytes, mime) = lookup_in::<TestAssets>("_next/static/x.js").unwrap();
        assert_eq!(mime, "text/javascript");
        assert!(String::from_utf8(bytes.into_owned()).unwrap().contains("fixture asset"));
    }

    #[test]
    fn missing_asset_returns_none() {
        assert!(lookup_in::<TestAssets>("missing.js").is_none());
    }

    #[test]
    fn unknown_extension_falls_back_to_octet_stream() {
        assert_eq!(mime_for("thing.unknownext"), "application/octet-stream");
    }
}
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test -p serrf-api --lib --features bundled-frontend static_assets::`
Expected: FAIL to compile — `lookup_in` and `mime_for` not found in this scope.

- [ ] **Step 6: Implement the module above the test block**

Add this above the `#[cfg(test)]` block in `crates/serrf-api/src/static_assets.rs`:

```rust
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "static-dist/"]
struct Assets;

pub fn lookup(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    lookup_in::<Assets>(path)
}

fn lookup_in<E: RustEmbed>(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let resolved = if path.is_empty() || !path.contains('.') {
        "index.html"
    } else {
        path
    };
    let file = E::get(resolved)?;
    Some((file.data, mime_for(resolved)))
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("json") | Some("map") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
```

- [ ] **Step 7: Add the feature-gated module declaration to `crates/serrf-api/src/lib.rs`**

```rust
pub mod app;
pub mod error;
pub mod job;
pub mod routes;
#[cfg(feature = "bundled-frontend")]
pub mod static_assets;
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p serrf-api --lib --features bundled-frontend static_assets::`
Expected: PASS, 5 tests.

- [ ] **Step 9: Confirm the default (feature-off) build is unaffected**

Run: `cargo build -p serrf-api && cargo test -p serrf-api --lib`
Expected: builds and passes exactly as before this task (the new module doesn't exist in this configuration).

- [ ] **Step 10: Commit**

```bash
git add crates/serrf-api/Cargo.toml crates/serrf-api/Cargo.lock crates/serrf-api/static-dist crates/serrf-api/tests/fixtures crates/serrf-api/src/static_assets.rs crates/serrf-api/src/lib.rs
git commit -m "Add bundled-frontend feature and static_assets embedded-lookup module"
```

---

### Task 2: Wire the static-file fallback route into `app.rs`

**Files:**
- Modify: `crates/serrf-api/src/app.rs`
- Create: `crates/serrf-api/tests/static_assets.rs`

**Interfaces:**
- Consumes: `serrf_api::static_assets::lookup` (Task 1).
- Produces: `GET /` (and any other unmatched path) returns the embedded static asset with a `Content-Type` header, or `404` if not found, only when compiled with `--features bundled-frontend`.

- [ ] **Step 1: Write the failing integration test**

Create `crates/serrf-api/tests/static_assets.rs`:

```rust
#![cfg(feature = "bundled-frontend")]

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
async fn root_serves_the_embedded_index_html() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/")).send().await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "text/html");
    let body = response.text().await.unwrap();
    assert!(body.contains("SERRF (dev placeholder)"));
}

#[tokio::test]
async fn unknown_path_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/does-not-exist")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p serrf-api --features bundled-frontend --test static_assets`
Expected: FAIL — `GET /` currently 404s (no fallback route registered yet), so both the status-code and content-type assertions on the first test fail.

- [ ] **Step 3: Add the fallback handler and wire it into `build_app()`**

Replace the full contents of `crates/serrf-api/src/app.rs` with:

```rust
#[derive(Clone)]
pub struct AppState {
    pub jobs: crate::job::JobStore,
}

pub fn build_app() -> axum::Router {
    let state = AppState {
        jobs: crate::job::JobStore::new(),
    };
    let router = axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id", axum::routing::get(crate::routes::status::status))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .route("/api/jobs/:id/result", axum::routing::get(crate::routes::result::result))
        .route("/api/jobs/:id/download", axum::routing::get(crate::routes::download::download))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

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
        Some((bytes, mime)) => {
            ([(axum::http::header::CONTENT_TYPE, mime)], bytes.into_owned()).into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p serrf-api --features bundled-frontend --test static_assets`
Expected: PASS, 2 tests.

- [ ] **Step 5: Run the full feature-on and feature-off suites to confirm nothing broke**

Run:
```bash
cargo test -p serrf-api --features bundled-frontend
cargo test -p serrf-api
cargo clippy -p serrf-api --all-targets --features bundled-frontend
cargo clippy -p serrf-api --all-targets
```
Expected: all four PASS/clean.

- [ ] **Step 6: Commit**

```bash
git add crates/serrf-api/src/app.rs crates/serrf-api/tests/static_assets.rs
git commit -m "Serve embedded static frontend assets as an axum fallback route"
```

---

### Task 3: Auto-open the default browser on startup

**Files:**
- Modify: `crates/serrf-api/Cargo.toml` (add optional `open` dependency, extend the feature)
- Modify: `crates/serrf-api/src/main.rs`

**Interfaces:**
- Produces: a pure, testable `serrf_api::browser_url(port: u16) -> String` helper; `main()` calls `open::that(...)` with it under `#[cfg(feature = "bundled-frontend")]`.
- Consumes: nothing new from earlier tasks.

Note on TDD scope here: `main()` itself (binding a socket, launching a real OS browser) is a side-effecting entry point with no existing test coverage today (same as `axum::serve(...).await.unwrap()` above it) — this project's convention is that `main.rs` stays thin and untested directly, while the logic that can be pure is pulled out and tested. `browser_url` is that pure, testable piece; actually invoking the OS browser opener is out of scope for automated tests (there is no browser or display on this machine to open one against).

- [ ] **Step 1: Add the optional dependency and extend the feature**

Run:
```bash
cargo add open --optional -p serrf-api
```

Update the `[features]` section in `crates/serrf-api/Cargo.toml`:

```toml
[features]
bundled-frontend = ["dep:rust-embed", "dep:open"]
```

- [ ] **Step 2: Write the failing test**

Add to the bottom of `crates/serrf-api/src/lib.rs` (just the test module — `browser_url` doesn't exist yet, so this won't compile):

```rust
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
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p serrf-api --lib --features bundled-frontend browser_url_tests::`
Expected: FAIL to compile — "cannot find function `browser_url` in this scope".

- [ ] **Step 4: Implement `browser_url` above the test module**

Add this above the `#[cfg(all(test, ...))]` block from Step 2:

```rust
#[cfg(feature = "bundled-frontend")]
pub fn browser_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p serrf-api --lib --features bundled-frontend browser_url_tests::`
Expected: PASS, 2 tests.

- [ ] **Step 6: Wire it into `main.rs`**

Replace the full contents of `crates/serrf-api/src/main.rs` with:

```rust
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
```

- [ ] **Step 7: Run the full feature-on and feature-off builds to confirm nothing broke**

Run:
```bash
cargo build -p serrf-api --features bundled-frontend
cargo build -p serrf-api
cargo test -p serrf-api --features bundled-frontend
cargo test -p serrf-api
```
Expected: all four PASS/build clean.

- [ ] **Step 8: Commit**

```bash
git add crates/serrf-api/Cargo.toml crates/serrf-api/Cargo.lock crates/serrf-api/src/main.rs crates/serrf-api/src/lib.rs
git commit -m "Auto-open the default browser on startup when bundled-frontend is enabled"
```

---

### Task 4: Frontend static export + Windows release script

**Files:**
- Modify: `frontend/next.config.js`
- Create: `scripts/build-windows-release.sh`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: the `bundled-frontend` feature and `static-dist/` embed target from Tasks 1–3.
- Produces: `dist/serrf-api-windows.exe` (gitignored build output) when the script is run.

- [ ] **Step 1: Confirm the pre-change baseline (no static export exists yet)**

Run: `ls frontend/out 2>&1`
Expected: `No such file or directory` — this is the "before" state the next step changes.

- [ ] **Step 2: Switch the frontend to a static export**

Modify `frontend/next.config.js`:

```js
/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "export",
  async rewrites() {
    const apiInternalUrl = process.env.API_INTERNAL_URL ?? "http://127.0.0.1:8080";
    return [{ source: "/api/:path*", destination: `${apiInternalUrl}/api/:path*` }];
  },
};

module.exports = nextConfig;
```

(The `rewrites()` block is inert for `next export`'s static output — Next.js only applies it in `next dev`/`next start` — so it's kept as-is for the existing dev-server workflow via `dev-start.sh` and is simply unused when this config produces a static export.)

- [ ] **Step 3: Run the frontend build and verify the export now exists**

Run: `cd frontend && npm run build`
Expected: succeeds, and `ls frontend/out/index.html` now exists (this is the "after" signal proving the config change took effect).

- [ ] **Step 4: Run the full existing frontend test suite to confirm the config change didn't break anything**

Run (from `frontend/`): `npm run lint && npx tsc --noEmit && npm test -- --run`
Expected: all PASS, matching the baseline established earlier in this session (9 test files, 25 tests).

- [ ] **Step 5: Write the release script**

Create `scripts/build-windows-release.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required (cross uses it to cross-compile for Windows). Install Docker and try again." >&2
  exit 1
fi

if ! command -v cross >/dev/null 2>&1; then
  echo "cross is required. Install it with:" >&2
  echo "  cargo install cross --git https://github.com/cross-rs/cross" >&2
  exit 1
fi

echo "==> Building the frontend static export"
(cd frontend && npm ci && npm run build)

echo "==> Copying the static export into crates/serrf-api/static-dist (temporarily replacing the placeholder)"
rm -rf crates/serrf-api/static-dist
mkdir -p crates/serrf-api/static-dist
cp -r frontend/out/. crates/serrf-api/static-dist/

echo "==> Cross-compiling serrf-api for x86_64-pc-windows-gnu"
cross build --release --target x86_64-pc-windows-gnu -p serrf-api --features bundled-frontend

echo "==> Restoring the committed static-dist placeholder"
git checkout -- crates/serrf-api/static-dist/

mkdir -p dist
cp target/x86_64-pc-windows-gnu/release/serrf-api.exe dist/serrf-api-windows.exe

size=$(du -h dist/serrf-api-windows.exe | cut -f1)
echo "==> Done: dist/serrf-api-windows.exe ($size)"
echo "    Hand this file to your colleague. Double-clicking it opens http://localhost:8080 automatically."
```

- [ ] **Step 6: Make the script executable**

Run: `chmod +x scripts/build-windows-release.sh`

- [ ] **Step 7: Add the build output directory to `.gitignore`**

Add to `.gitignore`:

```
/dist
```

- [ ] **Step 8: Run the script for real and verify the output binary**

Run: `./scripts/build-windows-release.sh`

This is the first real cross-compilation of the full workspace (including `serrf-core`'s `plotters`/`font-kit` dependency chain — see the Global Constraints risk note) for Windows; it will take a while the first time (`cross` pulls its Docker image, then compiles from scratch). If it fails on a `font-kit`/`dwrote` link error, stop here and report the exact error rather than guessing at a fix.

Once it succeeds, verify:

```bash
file dist/serrf-api-windows.exe
git status --porcelain -- crates/serrf-api/static-dist
```

Expected: `file` reports `PE32+ executable (console) x86-64, for MS Windows`; `git status` on `static-dist` is empty (the placeholder was restored cleanly, proving Step 5's `git checkout` step worked).

- [ ] **Step 9: Commit**

```bash
git add frontend/next.config.js scripts/build-windows-release.sh .gitignore
git commit -m "Add Windows release script: static frontend export + cross-compiled serrf-api.exe"
```

Do not `git add dist/` — it's gitignored build output, not source.

---

## Final verification (after all four tasks)

- [ ] Run the complete default (feature-off) suite once more: `cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --all -- --check` — must match the clean baseline from before this plan.
- [ ] Run the complete feature-on suite: `cargo test -p serrf-api --features bundled-frontend && cargo clippy -p serrf-api --all-targets --features bundled-frontend`.
- [ ] Confirm `dist/serrf-api-windows.exe` exists locally and was produced by re-running `scripts/build-windows-release.sh` (not committed to git).
