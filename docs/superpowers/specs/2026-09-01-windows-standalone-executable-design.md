# Windows Standalone Executable — Design

## Context

Plans 1–3 (`serrf-core`/`serrf-cli`, `serrf-api`, Next.js/MUI frontend) are merged to `master`.
Today, running the full app requires a Rust toolchain (`cargo run -p serrf-api`) and a Node
toolchain (`next dev`/`next start`), wired together via `dev-start.sh` and `next.config.js`'s
`/api/*` rewrite proxy. That's fine for development, but there's no way to hand the app to someone
without Docker, Rust, or Node knowledge.

`serrf-api`'s frontend calls already default to a same-origin relative base
(`apiBase()` in `frontend/src/lib/api.ts` returns `process.env.NEXT_PUBLIC_API_BASE ?? ""`), and
the frontend is a single route (`frontend/src/app/page.tsx`, no nested routes) with no
server-side rendering or API routes of its own — so serving it as a static export from the same
process that serves the API is a same-origin fit with zero frontend code changes.

The workspace has no OpenSSL/native-TLS/BLAS dependencies (`reqwest`, the only TLS-touching crate,
is a `serrf-api` **dev**-dependency only, used by integration tests, never shipped), so
cross-compiling to Windows carries low risk of native-toolchain pain.

The maintainer cannot rely on GitHub Actions for this (no new CI workflow) — the build must be a
local, repeatable script. Docker is available locally; `rustup` is not on `PATH`, so the script
uses `cross` (Docker-based cross-compilation) rather than installing a `mingw-w64`/rustup Windows
target locally.

## Goal

A single `serrf-api-windows.exe` that a non-technical Windows user can double-click, which opens
their browser to the running app — no Docker, no Rust, no Node required on their machine. Producing
that `.exe` must be repeatable by re-running one script on the maintainer's machine, with no new
GitHub Actions workflow.

## Architecture

### Feature-gated embed, not a new crate

`serrf-api` gains a new Cargo feature, `bundled-frontend`, **default off**. It changes nothing
about the crate's dependencies or routes unless enabled — `cargo build`/`cargo test -p serrf-api`
without the feature behaves exactly as today, so the existing dev loop (`dev-start.sh` + `next dev`
+ rewrite proxy) and existing CI (`cargo test`/`clippy`/`fmt` on every push) are unaffected.

When `bundled-frontend` is enabled:

- `crates/serrf-api/static-dist/` is a **committed** directory holding a small placeholder
  (`index.html`, e.g. "SERRF (dev placeholder — run `scripts/build-windows-release.sh` to embed the
  real frontend)"). It's embedded at compile time via `rust-embed`
  (`#[derive(RustEmbed)] #[folder = "static-dist/"]`) — real source, not gitignored, so a fresh
  clone can build and test the feature without ever running the release script. The release script
  (below) temporarily overwrites its contents with the real Next.js export to produce the actual
  Windows binary, then restores the placeholder afterward via `git checkout`.
- A new module, `crates/serrf-api/src/static_assets.rs`, defines a `lookup_in<E: RustEmbed>(path:
  &str) -> Option<(std::borrow::Cow<'static, [u8]>, &'static str)>` (bytes + MIME type, resolved
  from the file extension via a small hand-rolled match — html/js/css/json/png/jpg/svg/ico/woff/
  woff2/map, default `application/octet-stream`; no extra dependency needed for the handful of
  static-export file types) generic over any `RustEmbed` type, plus a thin
  `pub fn lookup(path: &str) -> ...` wrapper bound to the real `static-dist/`-backed type. `path` of
  `""` resolves to `index.html`; any other unmatched path 404s (this is a single-route static
  export, so no client-side-routing fallback is needed). Note: the day the frontend gains a second
  real route, deep links to non-existent-but-should-exist paths would need a client-side-routing
  fallback added. The generic form lets tests exercise the same logic against a small, permanent
  test fixture directory instead of depending on `static-dist/`'s placeholder-vs-real-export state.
- `crates/serrf-api/src/app.rs`'s `build_app()` adds one fallback handler, gated by
  `#[cfg(feature = "bundled-frontend")]`, that calls `static_assets::lookup` for any path not
  matched by the existing `/health`/`/api/*` routes and returns 404 if `lookup` returns `None`.

### Build-time asset pipeline

`next.config.js` switches permanently to `output: "export"` (there's no SSR/API-route usage to
lose). `npm run build` then produces `frontend/out/`, a plain static directory, which the release
script copies into `crates/serrf-api/static-dist/` before compiling. This copy step (not pointing
`rust-embed` at `../../frontend/out` directly) keeps `serrf-api` buildable in isolation and matches
the existing pattern of not reaching outside crate boundaries.

### Startup UX

`crates/serrf-api/src/main.rs`, under `#[cfg(feature = "bundled-frontend")]`, opens
`http://127.0.0.1:<port>` in the OS's default browser after the listener binds, using the `open`
crate (shells out to the OS opener — `cmd /c start` on Windows — no native build dependencies).
Feature-off builds keep today's behavior (print the listening address, no browser launch). Port
selection is unchanged from today: `PORT` env var, default `8080`.

## Build & release process

A new script, `scripts/build-windows-release.sh`, run manually and repeatably by the maintainer
(no GitHub Actions involvement):

1. Check `docker` and `cross` are on `PATH`; if `cross` is missing, fail fast with
   `cargo install cross --git https://github.com/cross-rs/cross`.
2. `cd frontend && npm ci && npm run build` — produces `frontend/out/`.
3. `rm -rf crates/serrf-api/static-dist && mkdir -p crates/serrf-api/static-dist && cp -r frontend/out/* crates/serrf-api/static-dist/`.
4. `cross build --release --target x86_64-pc-windows-gnu -p serrf-api --features bundled-frontend`.
5. `mkdir -p dist && cp target/x86_64-pc-windows-gnu/release/serrf-api.exe dist/serrf-api-windows.exe`.
6. `git checkout -- crates/serrf-api/static-dist/` — restores the committed placeholder so the
   working tree is clean again after the release build (the real Next.js export was only ever a
   transient overwrite for the `cross build` step).
7. Print the output file's size and a one-line usage reminder ("double-click it, then it opens
   `http://localhost:8080` automatically").

`dist/` is added to `.gitignore` — it's build output, not source. `crates/serrf-api/static-dist/`
stays fully tracked (see above).

## Testing strategy

Per standing workflow: TDD, real deps, no mocks.

- **Unit**: `lookup_in::<E>`, tested against a small permanent fixture directory
  (`crates/serrf-api/tests/fixtures/static/index.html` + one nested asset, e.g.
  `_next/static/x.js`) embedded via its own test-only `#[derive(RustEmbed)]` — this is a real
  embedded-asset lookup, not a mock, and doesn't depend on `static-dist/`'s placeholder-vs-real
  state. Write the failing tests first: `lookup_in("")` and `lookup_in("index.html")` both resolve
  to the fixture's `index.html` bytes with MIME `text/html`; `lookup_in("_next/static/x.js")`
  resolves with MIME `text/javascript`; `lookup_in("missing.js")` returns `None`.
- **Integration**: an axum test in `crates/serrf-api/tests/static_assets.rs`, compiled only under
  `--features bundled-frontend`, spawning the real app (same `spawn_app()` pattern as
  `tests/upload.rs`/`tests/events.rs`) against the real, committed `static-dist/` placeholder,
  asserting `GET /` returns 200 with a body containing the placeholder's known text and
  `GET /does-not-exist` returns 404. `/api/*` and `/health` routes continue to be covered by their
  existing tests, unaffected by this feature.
- **Build script**: not unit-testable in the TDD sense (it shells out to Docker and produces a
  Windows PE binary that can't run on this Linux machine — no `wine` is installed here). The
  verification available is running the script for real and confirming `file dist/serrf-api-windows.exe`
  reports a valid `PE32+ executable (console) x86-64, for MS Windows`. Actual functional
  verification (double-click, browser opens, upload/normalize/download works) happens the first
  time it's run on a real Windows machine — this will be called out explicitly rather than claimed
  as tested.

## Out of scope

- Auto-updating the `.exe`, code-signing, or an installer/MSI — this is a single portable binary.
- Picking a free port automatically or handling a "port already in use" error gracefully beyond
  today's existing `.unwrap()` on bind — not part of this ask; can be a fast-follow if it becomes a
  real problem.
- A native window (Tauri/Electron) — explicitly decided against in favor of the simpler
  embedded-binary-plus-browser-tab approach.
- Any change to the existing dev workflow, CI, or non-Windows platforms.
