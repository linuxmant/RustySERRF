# SERRF Port Plan 3: Next.js/MUI Frontend — Design

## Context

Plans 1 (`serrf-core`/`serrf-cli`) and 2 (`serrf-api`) are merged to `master`. `serrf-api` is a
running axum server exposing:

- `POST /api/jobs` — multipart upload, starts a background normalization job, returns `{job_id}`.
- `GET /api/jobs/:id/events` — SSE stream of `JobEvent` (`Queued | Progress{stage,current,total} |
  Completed | Failed{error}`).
- `GET /api/jobs/:id/result` — JSON summary (`ResultJson`): compound labels, QC-RSD arrays
  (raw/SERRF, plus per-validate-type), and before/after PCA coordinates (`pc1`/`pc2` only, today).
- `GET /api/jobs/:id/download` — zip of normalized CSVs, `QC-RSDs.csv`, and a server-rendered PNG.

CORS is permissive (merged via PR #3) since there's no auth boundary in this API and production
runs same-origin behind a reverse proxy per the original design spec
(`docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md`).

No `frontend/` directory exists yet — this is new subsystem work. This spec covers Plan 3: the
Next.js/MUI frontend, plus two small `serrf-api` additions this frontend needs.

## Goal

Full parity with the current R/Shiny UI's user-facing flow: upload a dataset, watch normalization
progress, view QC-RSD and before/after PCA charts, download the results zip — with dark and light
theme support from day one, per standing preference.

## API changes (done first, small and backward-compatible)

1. **`GET /api/jobs/:id`** — a new status-only endpoint returning the same shape as the SSE
   `JobEvent` (`{status: "queued"|"progress"|"completed"|"failed", stage?, current?, total?,
   error?}`), read directly from the existing in-memory `JobEvent` watch value. No result
   computation, so it's cheap to poll. Motivated by Plan 2's final review: lets the frontend do a
   one-shot reconnect/fallback check without hitting the expensive `/result` endpoint.
2. **Extend `PcaJson`** (in `routes/result.rs`) with `sample_type: Vec<Option<String>>` and
   `batch: Vec<String>` parallel arrays on both `pca_before` and `pca_after`, so the frontend can
   color/legend PCA scatter points by sample type and batch — matching what the R app's PCA plot
   showed. Requires threading `batch` through `CompletedJob` (in `job.rs`) the same way
   `sample_type` is already threaded from `Dataset.samples.columns["batch"]` at upload time.

Both follow the existing `serrf-api` TDD pattern: real HTTP integration tests against a running
axum instance, no mocks.

## Frontend architecture

```
frontend/
├── src/
│   ├── app/
│   │   ├── layout.tsx          # MUI ThemeProvider, dark/light toggle, CssBaseline
│   │   ├── page.tsx            # single route: renders the state machine below
│   │   └── theme.ts            # MUI theme (light + dark palettes)
│   ├── components/
│   │   ├── UploadForm.tsx      # file picker/drag-drop, submits to POST /api/jobs
│   │   ├── ProgressView.tsx    # SSE-driven progress bar + stage text
│   │   ├── ResultsView.tsx     # orchestrates the result panels below
│   │   ├── RsdBarChart.tsx     # MUI X Charts bar: QC-RSD raw vs SERRF per compound
│   │   ├── PcaScatter.tsx      # MUI X Charts scatter: before/after, colored by sample_type/batch
│   │   ├── DownloadButton.tsx  # links to GET /api/jobs/:id/download
│   │   └── ThemeToggle.tsx
│   ├── lib/
│   │   ├── api.ts              # typed fetch wrappers for all endpoints + SSE subscription helper
│   │   └── types.ts            # TS types mirroring ResultJson/JobEvent (hand-kept in sync)
│   └── hooks/
│       └── useJob.ts           # state machine: idle -> uploading -> processing -> done|error
├── e2e/
│   └── full-run.spec.ts        # Playwright: upload -> progress -> charts -> download -> theme
├── next.config.js              # rewrites: /api/* -> http://localhost:8080/api/* (SERRF_API_URL)
├── playwright.config.ts        # webServer: boots `cargo run -p serrf-api` + `next start`
└── package.json                # npm
```

**State machine (`useJob` hook)** drives the single page: `Idle` (show `UploadForm`) → on submit,
`POST /api/jobs` → `Processing{jobId}` (show `ProgressView`, subscribed via `EventSource` to
`/api/jobs/:id/events`, with `GET /api/jobs/:id` as a one-shot reconnect/fallback check if the SSE
connection drops) → on `Completed` event, fetch `GET /api/jobs/:id/result` → `Done{result}` (show
`ResultsView`) → on `Failed` event or any fetch error → `Error{message}` with a "start over" action
back to `Idle`.

No global state library (Zustand/Redux) — this is one linear flow on one page; a single hook with
a discriminated-union state type covers it per YAGNI.

**Types note**: there's no shared schema/codegen between Rust and TypeScript in this repo, so
`types.ts` is hand-maintained to mirror the Rust JSON shapes (`ResultJson` in `result.rs`,
`JobEvent` in `job.rs`). Same risk profile as today's OpenAPI-less setup — acceptable for a
single-host research tool per the original spec's scope decisions.

## UI content per view (parity target, not a redesign)

- **Upload**: file input (`.xlsx`/`.csv`) + submit button. On `400` validation errors from the API,
  show the structured error message(s) inline (e.g. "batch X has fewer than 6 QC samples") rather
  than a generic failure.
- **Progress**: linear progress bar computed from `current/total`, stage text (e.g. "SERRF
  normalization: 142/268"), matching the granularity of the R app's `incProgress` calls.
- **Results**:
  - RSD bar chart: raw vs. SERRF median RSD per compound (mirrors the R barplot panel).
  - Two PCA scatter plots (before/after), points colored by `sample_type`, legend includes batch
    when multiple batches are present.
  - A lightweight summary panel (compound count, overall median RSD raw vs. SERRF) — not a full
    data grid.
  - Download button → `GET /api/jobs/:id/download` (zip), plus a "start a new run" action
    resetting to `Idle`.
- **Theme toggle**: persisted via `localStorage`, respects `prefers-color-scheme` on first load.
  Both light and dark are the standing default; the Playwright E2E suite exercises the toggle
  explicitly.

## Testing strategy

Per standing workflow: TDD, 4 layers, 80%+ coverage (frontend's own `src/`), no mocks/stubs/fakes
except where noted below.

- **Unit (Vitest + React Testing Library)**: `useJob` state machine transitions (mocking only the
  network boundary — `fetch`/`EventSource` — the one necessary bend on "no mocks" for a frontend,
  since a hook can't be unit-tested against a real server without becoming an integration test);
  chart components render given fixed data; theme toggle persists to `localStorage`.
- **Integration**: component tests mounting `UploadForm` → `ProgressView` → `ResultsView` against a
  **real running `serrf-api`** instance (started once per test file, real HTTP + real SSE, no
  fetch mocking) — this is the layer that actually honors "real deps only" for this feature.
- **E2E (Playwright)**: full browser run — upload the bundled example dataset, watch the progress
  bar move, verify both charts render with data, download the zip and assert it's a valid
  non-empty archive, toggle dark/light theme and verify persisted state.
- **Smoke**: extend the existing smoke test / `dev-start.sh` check to also hit the frontend's `/`
  and confirm it 200s once both processes are up.
- **serrf-api additions**: the new `GET /api/jobs/:id` endpoint and the extended `PcaJson` fields
  get their own real-HTTP integration tests in `serrf-api`, following the existing pattern.

## Deployment

Single Docker image, two processes — the existing `serrf-api` release binary and `next start`
(production build) — with `next.config.js` rewrites proxying `/api/*` to the axum process on an
internal port (e.g. `127.0.0.1:8080`), so only Next.js's port is exposed externally.
`dev-start.sh`/`dev-stop.sh` updated to boot both processes locally, idempotent via `.dev.pid` per
existing convention. CORS stays permissive (already merged) — it's what makes local dev (`next dev`
on `:3000` talking directly to axum on `:8080`, bypassing the rewrite) painless; production traffic
goes through the rewrite so it's same-origin regardless.

## Out of scope for this milestone

Consistent with the original design spec's parity-first framing:

- No auth, no persistent job history.
- No multi-job dashboard — one job in flight at a time per the single-page state machine.
- No i18n.
- No dedicated mobile layout — responsive-enough via MUI's grid, not a separate design pass.
- No client-side data editing/re-upload-without-refresh beyond the "start a new run" reset.
- Shared Rust/TypeScript schema codegen (e.g. via `ts-rs` or an OpenAPI spec) — hand-maintained
  types are accepted for now; revisit if the hand-sync becomes a recurring source of bugs.
