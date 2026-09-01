# SERRF Frontend (Plan 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Next.js/MUI frontend for RustySERRF — upload a dataset, watch SERRF normalization progress over SSE, view QC-RSD and before/after PCA charts, download the results zip — with dark/light theme support, plus two small `serrf-api` additions the frontend needs.

**Architecture:** A new `frontend/` Next.js (App Router, TypeScript) app talks to the existing `serrf-api` axum server. In dev, `next dev` on `:3000` calls axum on `:8080` directly (CORS is already permissive). In production, `next.config.js` rewrites proxy `/api/*` server-side to axum so only Next.js's port is exposed. A single `useJob` hook drives a one-page state machine (`idle -> uploading -> processing -> done|error`); SSE progress comes from `GET /api/jobs/:id/events`, with the new `GET /api/jobs/:id` status endpoint as a one-shot reconnect fallback. Charts use MUI X Charts. No global state library — one hook, one page.

**Tech Stack:** Next.js 15 (App Router), React 19, TypeScript 5 (strict), MUI 6 (`@mui/material`, `@mui/x-charts`, `@mui/material-nextjs` for App Router Emotion SSR), Vitest 2 + React Testing Library (unit/component tests), Playwright 1.48 (E2E). Backend additions use the existing `axum`/`serde` stack already in `serrf-api`.

**Spec:** `docs/superpowers/specs/2026-09-01-serrf-frontend-design.md` — this plan implements that spec's API-changes, frontend-architecture, UI-content, testing-strategy, and deployment sections. The original port spec, `docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md`, is the parent design (Next.js/MUI choice, dark+light from day one).

## Global Constraints

- TDD non-negotiable: write the failing test before implementation code, for every task below.
- No mocks/stubs/fakes, with one accepted bend documented in the spec: pure-unit tests of `lib/api.ts` and `hooks/useJob.ts` mock the network boundary (`fetch`/`EventSource`) or the `lib/api` module respectively — every other layer (component-vs-real-backend integration test, Playwright E2E) uses a real running `serrf-api`.
- 80%+ coverage (statements, branches, functions, lines) for the frontend's own `src/` code — check with `npm run test:coverage` before considering the plan done. `serrf-api`'s existing `cargo tarpaulin` gate covers the two backend tasks.
- Commit after every task (not every step) — one focused commit per task, on branch `feat/serrf-frontend` (create this branch from the tip of `master` before Task 1).
- `serrf_core::normalize` remains synchronous/CPU-bound and must keep running inside `tokio::task::spawn_blocking` — the two backend tasks below don't touch that call, only add a cheap status read and thread one extra field through the already-completed job.
- Package manager: npm (`frontend/package-lock.json` is committed).
- The Next.js app lives in `frontend/` at the repo root, independent of the Cargo workspace.
- Out of scope for this plan (per spec): auth, persistent job history, multi-job dashboard, i18n, dedicated mobile layout, Rust/TypeScript schema codegen.

---

### Task 1: `GET /api/jobs/:id` status endpoint (`serrf-api`)

**Files:**
- Create: `crates/serrf-api/src/routes/status.rs`
- Create: `crates/serrf-api/tests/status.rs`
- Modify: `crates/serrf-api/src/routes/mod.rs`
- Modify: `crates/serrf-api/src/app.rs`

**Interfaces:**
- Produces: `GET /api/jobs/:id` — `200` JSON, the exact serde shape of `crate::job::JobEvent` (`{"status":"queued"}` / `{"status":"progress","stage":...,"current":...,"total":...}` / `{"status":"completed"}` / `{"status":"failed","error":...}`); `404` unknown job id; `400` malformed id. Reads the job's current `watch` value directly — no lock held across the response, no result computation.
- Consumes: `crate::job::{JobStore, JobId, JobEvent}` (existing, `job.rs`), `crate::error::ApiError` (existing, `error.rs`).

- [ ] **Step 1: Write the failing integration test**

Create `crates/serrf-api/tests/status.rs`:

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
async fn status_eventually_reports_completed() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let job_id = upload_fixture(&base_url, &client).await;

    for _ in 0..100 {
        let response = client.get(format!("{base_url}/api/jobs/{job_id}")).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let body: serde_json::Value = response.json().await.unwrap();
        if body["status"] == "completed" {
            return;
        }
        assert!(
            body["status"] == "queued" || body["status"] == "progress",
            "unexpected status while waiting: {body}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job never reported completed within the polling window");
}

#[tokio::test]
async fn status_for_an_unknown_job_returns_404() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let fake_id = uuid::Uuid::new_v4();

    let response = client.get(format!("{base_url}/api/jobs/{fake_id}")).send().await.unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn status_for_a_malformed_job_id_returns_400() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();

    let response = client.get(format!("{base_url}/api/jobs/not-a-uuid")).send().await.unwrap();

    assert_eq!(response.status(), 400);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p serrf-api --test status`
Expected: FAIL — `GET /api/jobs/:id` doesn't exist yet, so requests 404 (making the "unknown job" test pass but the other two fail) or return the wrong body shape.

- [ ] **Step 3: Implement `crates/serrf-api/src/routes/status.rs`**

```rust
use crate::app::AppState;
use crate::error::ApiError;
use crate::job::{JobEvent, JobId};
use axum::extract::{Path, State};
use axum::Json;

pub async fn status(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<JobEvent>, ApiError> {
    let job_id = JobId::parse(&id).map_err(|_| ApiError::BadRequest("invalid job id".to_string()))?;
    let rx = state.jobs.subscribe(job_id).ok_or(ApiError::NotFound)?;
    Ok(Json(rx.borrow().clone()))
}
```

- [ ] **Step 4: Add `pub mod status;` to `crates/serrf-api/src/routes/mod.rs`**

```rust
pub mod download;
pub mod events;
pub mod result;
pub mod status;
pub mod upload;
```

- [ ] **Step 5: Wire the route into `crates/serrf-api/src/app.rs`**

```rust
pub fn build_app() -> axum::Router {
    let state = AppState {
        jobs: crate::job::JobStore::new(),
    };
    axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/jobs", axum::routing::post(crate::routes::upload::upload))
        .route("/api/jobs/:id", axum::routing::get(crate::routes::status::status))
        .route("/api/jobs/:id/events", axum::routing::get(crate::routes::events::events))
        .route("/api/jobs/:id/result", axum::routing::get(crate::routes::result::result))
        .route("/api/jobs/:id/download", axum::routing::get(crate::routes::download::download))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state)
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p serrf-api --test status`
Expected: PASS, 3 tests.

- [ ] **Step 7: Run the full fast workspace suite to confirm nothing broke**

Run: `cargo test --workspace --exclude serrf-api -- && cargo test -p serrf-api --test health --test upload --test events --test download --test cors --test failure_path --test status`
Expected: PASS, no failures. (Skip `golden_e2e` here — it's slow and unrelated to this change.)

- [ ] **Step 8: Commit**

```bash
git add crates/serrf-api/src/routes/status.rs crates/serrf-api/src/routes/mod.rs crates/serrf-api/src/app.rs crates/serrf-api/tests/status.rs
git commit -m "Add GET /api/jobs/:id status endpoint"
```

---

### Task 2: Extend `PcaJson` with `sample_type`/`batch` (`serrf-api`)

**Files:**
- Modify: `crates/serrf-api/src/job.rs` (add `batch: Vec<String>` to `CompletedJob`, update the `sample_completed()` test fixture)
- Modify: `crates/serrf-api/src/routes/upload.rs` (capture `samples.batch.clone()` into the new `CompletedJob` field)
- Modify: `crates/serrf-api/src/routes/result.rs` (extend `PcaJson`, populate from `completed.sample_type`/`completed.batch`)
- Modify: `crates/serrf-api/tests/upload.rs` (assert the new fields appear in a real `/result` response)

**Interfaces:**
- Produces: `PcaJson` gains `sample_type: Vec<Option<String>>` and `batch: Vec<String>`, both parallel in length/order to `pc1`/`pc2` (one entry per sample, same order as `serrf_core::pca::pca_first_two`'s input columns). `CompletedJob` gains `pub batch: Vec<String>`.
- Consumes: `serrf_core::validate::ValidatedSamples` (existing, has `pub batch: Vec<String>` already — see `crates/serrf-core/src/validate.rs:8`).

- [ ] **Step 1: Write the failing integration test**

Add to `crates/serrf-api/tests/upload.rs` (reuses that file's existing `spawn_app`/`valid_csv_fixture` helpers):

```rust
#[tokio::test]
async fn result_pca_includes_sample_type_and_batch_per_point() {
    let base_url = spawn_app().await;
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::text(valid_csv_fixture()).file_name("dataset.csv").mime_str("text/csv").unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let response = client.post(format!("{base_url}/api/jobs")).multipart(form).send().await.unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let job_id = body["job_id"].as_str().unwrap().to_string();

    let result = loop {
        let response = client.get(format!("{base_url}/api/jobs/{job_id}/result")).send().await.unwrap();
        if response.status() != 425 {
            assert_eq!(response.status(), 200);
            break response.json::<serde_json::Value>().await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };

    let pca_before = &result["pca_before"];
    let n_points = pca_before["pc1"].as_array().unwrap().len();
    assert_eq!(pca_before["sample_type"].as_array().unwrap().len(), n_points);
    assert_eq!(pca_before["batch"].as_array().unwrap().len(), n_points);
    assert!(pca_before["batch"].as_array().unwrap().iter().any(|b| b == "A"));
    assert!(pca_before["batch"].as_array().unwrap().iter().any(|b| b == "B"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p serrf-api --test upload result_pca_includes`
Expected: FAIL — `pca_before["sample_type"]`/`["batch"]` are `null`, so `.as_array().unwrap()` panics.

- [ ] **Step 3: Add `batch` to `CompletedJob` and its test fixture in `crates/serrf-api/src/job.rs`**

```rust
pub struct CompletedJob {
    pub compound_labels: Vec<String>,
    pub sample_type: Vec<Option<String>>,
    pub batch: Vec<String>,
    pub output: serrf_core::PipelineOutput,
}
```

Update `sample_completed()` in the same file's `#[cfg(test)] mod tests`:

```rust
fn sample_completed() -> CompletedJob {
    CompletedJob {
        compound_labels: vec!["c1".to_string()],
        sample_type: vec![Some("qc".to_string())],
        batch: vec!["A".to_string()],
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
```

- [ ] **Step 4: Populate `batch` at the construction site in `crates/serrf-api/src/routes/upload.rs`**

`samples.batch` already exists on `ValidatedSamples` (`crates/serrf-core/src/validate.rs:8`) — capture it alongside the existing `sample_type` clone, and add it to the `CompletedJob` literal:

```rust
let compound_labels = dataset.compounds.label.clone();
let sample_type = samples.sample_type.clone();
let batch = samples.batch.clone();
```

```rust
Ok(output) => jobs.complete(
    job_id,
    crate::job::CompletedJob {
        compound_labels,
        sample_type,
        batch,
        output,
    },
),
```

- [ ] **Step 5: Extend `PcaJson` and its population in `crates/serrf-api/src/routes/result.rs`**

```rust
#[derive(serde::Serialize)]
pub struct PcaJson {
    pc1: Vec<f64>,
    pc2: Vec<f64>,
    sample_type: Vec<Option<String>>,
    batch: Vec<String>,
}
```

```rust
pca_before: PcaJson {
    pc1: pca_before.pc1,
    pc2: pca_before.pc2,
    sample_type: completed.sample_type.clone(),
    batch: completed.batch.clone(),
},
pca_after: PcaJson {
    pc1: pca_after.pc1,
    pc2: pca_after.pc2,
    sample_type: completed.sample_type.clone(),
    batch: completed.batch.clone(),
},
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p serrf-api --test upload`
Expected: PASS, all tests in the file including the new one.

- [ ] **Step 7: Run the fast workspace suite to confirm nothing broke**

Run: `cargo test --workspace --exclude serrf-api -- && cargo test -p serrf-api --test health --test upload --test events --test download --test cors --test failure_path --test status`
Expected: PASS, no failures.

- [ ] **Step 8: Commit**

```bash
git add crates/serrf-api/src/job.rs crates/serrf-api/src/routes/upload.rs crates/serrf-api/src/routes/result.rs crates/serrf-api/tests/upload.rs
git commit -m "Extend PcaJson with per-point sample_type and batch"
```

---

### Task 3: Scaffold the `frontend/` Next.js + TypeScript + MUI app with a Vitest harness

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/tsconfig.json`
- Create: `frontend/next.config.js`
- Create: `frontend/next-env.d.ts`
- Create: `frontend/.eslintrc.json`
- Create: `frontend/.gitignore`
- Create: `frontend/vitest.config.ts`
- Create: `frontend/vitest.setup.ts`
- Create: `frontend/src/app/layout.tsx`
- Create: `frontend/src/app/page.tsx`
- Create: `frontend/src/app/globals.css`
- Create: `frontend/src/app/page.test.tsx`

**Interfaces:**
- Produces: a working `npm run dev`/`npm run build`/`npm test` app with a placeholder home page, ready for later tasks to fill in. `next.config.js`'s rewrite (`/api/:path*` -> `${API_INTERNAL_URL ?? "http://127.0.0.1:8080"}/api/:path*`) is wired from the start since it's one line and later tasks depend on it existing.
- Consumes: nothing yet.

- [ ] **Step 1: Write the failing smoke test**

Create `frontend/src/app/page.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import Home from "./page";

describe("Home", () => {
  it("renders the app title", () => {
    render(<Home />);
    expect(screen.getByRole("heading", { name: "RustySERRF" })).toBeInTheDocument();
  });
});
```

(This step's "run it to see it fail" is Step 8 below, after the toolchain exists to run it at all — there's no prior test runner in `frontend/` yet.)

- [ ] **Step 2: Create `frontend/package.json`**

```json
{
  "name": "serrf-frontend",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "dev": "next dev",
    "build": "next build",
    "start": "next start",
    "lint": "next lint",
    "typecheck": "tsc --noEmit",
    "test": "vitest run src",
    "test:coverage": "vitest run src --coverage",
    "test:integration": "vitest run tests/integration",
    "e2e": "playwright test"
  },
  "dependencies": {
    "@emotion/react": "^11.13.0",
    "@emotion/styled": "^11.13.0",
    "@mui/material": "^6.1.0",
    "@mui/material-nextjs": "^6.1.0",
    "@mui/x-charts": "^7.18.0",
    "next": "^15.0.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@playwright/test": "^1.48.0",
    "@testing-library/jest-dom": "^6.5.0",
    "@testing-library/react": "^16.0.0",
    "@testing-library/user-event": "^14.5.0",
    "@types/node": "^22.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "@vitest/coverage-v8": "^2.1.0",
    "eslint": "^8.57.0",
    "eslint-config-next": "^15.0.0",
    "jsdom": "^25.0.0",
    "typescript": "^5.6.0",
    "vitest": "^2.1.0"
  }
}
```

- [ ] **Step 3: Create `frontend/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2017",
    "lib": ["dom", "dom.iterable", "esnext"],
    "allowJs": false,
    "skipLibCheck": true,
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "jsx": "preserve",
    "incremental": true,
    "plugins": [{ "name": "next" }]
  },
  "include": ["next-env.d.ts", "**/*.ts", "**/*.tsx", ".next/types/**/*.ts"],
  "exclude": ["node_modules"]
}
```

- [ ] **Step 4: Create `frontend/next-env.d.ts`, `.eslintrc.json`, `.gitignore`**

`frontend/next-env.d.ts`:

```ts
/// <reference types="next" />
/// <reference types="next/image-types/global" />
```

`frontend/.eslintrc.json`:

```json
{
  "extends": ["next/core-web-vitals", "next/typescript"]
}
```

`frontend/.gitignore`:

```
node_modules
.next
coverage
playwright-report
test-results
```

- [ ] **Step 5: Create `frontend/next.config.js`**

```js
/** @type {import('next').NextConfig} */
const nextConfig = {
  async rewrites() {
    const apiInternalUrl = process.env.API_INTERNAL_URL ?? "http://127.0.0.1:8080";
    return [{ source: "/api/:path*", destination: `${apiInternalUrl}/api/:path*` }];
  },
};

module.exports = nextConfig;
```

- [ ] **Step 6: Create `frontend/vitest.config.ts` and `frontend/vitest.setup.ts`**

```ts
// vitest.config.ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./vitest.setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/app/layout.tsx"],
      thresholds: { statements: 80, branches: 80, functions: 80, lines: 80 },
    },
  },
});
```

```ts
// vitest.setup.ts
import "@testing-library/jest-dom/vitest";

// MUI X Charts measures its container via ResizeObserver, which jsdom does not implement.
// Without this stub, any test rendering a chart (Tasks 9-11) throws "ResizeObserver is not defined".
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// eslint-disable-next-line @typescript-eslint/no-explicit-any
(globalThis as any).ResizeObserver ??= ResizeObserverStub;
```

- [ ] **Step 7: Create the placeholder app shell**

`frontend/src/app/globals.css`:

```css
html,
body {
  padding: 0;
  margin: 0;
}
```

`frontend/src/app/layout.tsx`:

```tsx
import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "RustySERRF",
  description: "SERRF normalization for metabolomics data",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
```

`frontend/src/app/page.tsx`:

```tsx
export default function Home() {
  return (
    <main>
      <h1>RustySERRF</h1>
    </main>
  );
}
```

- [ ] **Step 8: Install dependencies and run the test to verify RED then GREEN**

Run: `cd frontend && npm install`
Run: `npm test`
Expected: PASS, 1 test. (There is no meaningful RED step here beyond "the toolchain doesn't exist yet" — this task's TDD cycle is: write the test against the not-yet-created `Home`, then create the minimal `page.tsx` that satisfies it. Confirm this by temporarily reverting `page.tsx`'s `<h1>` text to something else and re-running `npm test` to see it fail, then restoring it.)

- [ ] **Step 9: Run typecheck, lint, and build to confirm the scaffold is sound**

Run: `npm run typecheck && npm run lint && npm run build`
Expected: all three PASS with no errors.

- [ ] **Step 10: Commit**

```bash
git add frontend
git commit -m "Scaffold frontend/: Next.js + TypeScript + Vitest harness"
```

---

### Task 4: MUI theme with dark/light toggle

**Files:**
- Create: `frontend/src/app/theme.ts`
- Create: `frontend/src/app/ThemeRegistry.tsx`
- Create: `frontend/src/components/ThemeToggle.tsx`
- Create: `frontend/src/components/ThemeToggle.test.tsx`
- Modify: `frontend/src/app/layout.tsx` (wrap children in `ThemeRegistry`)
- Modify: `frontend/src/app/page.tsx` (render `ThemeToggle` next to the title)
- Modify: `frontend/src/app/page.test.tsx` (the heading is now inside MUI markup — assert on role/name only, already compatible)

**Interfaces:**
- Produces: `frontend/src/app/theme.ts` exports `getTheme(mode: "light" | "dark"): Theme`. `ThemeRegistry` (client component) exports a default component wrapping `children`, and a named `ColorModeContext` (`{ mode: "light" | "dark"; toggle: () => void }`) other components can `useContext` to read/flip the mode. `ThemeToggle` is a self-contained button reading/writing that context and persisting to `localStorage` under key `"color-mode"`.
- Consumes: nothing from earlier tasks besides the scaffold.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/ThemeToggle.test.tsx`:

```tsx
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ThemeRegistry from "../app/ThemeRegistry";
import ThemeToggle from "./ThemeToggle";

describe("ThemeToggle", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to light mode and persists a switch to dark on click", async () => {
    render(
      <ThemeRegistry>
        <ThemeToggle />
      </ThemeRegistry>
    );
    const button = screen.getByRole("button", { name: /toggle theme/i });
    expect(localStorage.getItem("color-mode")).toBeNull();

    await userEvent.click(button);

    expect(localStorage.getItem("color-mode")).toBe("dark");
  });

  it("initializes from a previously persisted mode", () => {
    localStorage.setItem("color-mode", "dark");
    render(
      <ThemeRegistry>
        <ThemeToggle />
      </ThemeRegistry>
    );
    expect(screen.getByRole("button", { name: /toggle theme/i })).toHaveAttribute("aria-pressed", "true");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- ThemeToggle`
Expected: FAIL to compile — `../app/ThemeRegistry` and `./ThemeToggle` don't exist yet.

- [ ] **Step 3: Create `frontend/src/app/theme.ts`**

```ts
import { createTheme, type Theme } from "@mui/material/styles";

export function getTheme(mode: "light" | "dark"): Theme {
  return createTheme({ palette: { mode } });
}
```

- [ ] **Step 4: Create `frontend/src/app/ThemeRegistry.tsx`**

```tsx
"use client";

import { createContext, useEffect, useMemo, useState } from "react";
import CssBaseline from "@mui/material/CssBaseline";
import { ThemeProvider } from "@mui/material/styles";
import { AppRouterCacheProvider } from "@mui/material-nextjs/v14-appRouter";
import { getTheme } from "./theme";

export type ColorMode = "light" | "dark";

export const ColorModeContext = createContext<{ mode: ColorMode; toggle: () => void }>({
  mode: "light",
  toggle: () => {},
});

export default function ThemeRegistry({ children }: { children: React.ReactNode }) {
  const [mode, setMode] = useState<ColorMode>("light");

  useEffect(() => {
    const stored = localStorage.getItem("color-mode");
    if (stored === "light" || stored === "dark") {
      setMode(stored);
    } else if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
      setMode("dark");
    }
  }, []);

  const contextValue = useMemo(
    () => ({
      mode,
      toggle: () => {
        setMode((current) => {
          const next = current === "light" ? "dark" : "light";
          localStorage.setItem("color-mode", next);
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

- [ ] **Step 5: Create `frontend/src/components/ThemeToggle.tsx`**

```tsx
"use client";

import { useContext } from "react";
import IconButton from "@mui/material/IconButton";
import Brightness4Icon from "@mui/icons-material/Brightness4";
import Brightness7Icon from "@mui/icons-material/Brightness7";
import { ColorModeContext } from "../app/ThemeRegistry";

export default function ThemeToggle() {
  const { mode, toggle } = useContext(ColorModeContext);

  return (
    <IconButton aria-label="toggle theme" aria-pressed={mode === "dark"} onClick={toggle}>
      {mode === "dark" ? <Brightness7Icon /> : <Brightness4Icon />}
    </IconButton>
  );
}
```

Add `@mui/icons-material` to `frontend/package.json`'s `dependencies` (same version line as `@mui/material`, `"^6.1.0"`) and run `npm install`.

- [ ] **Step 6: Wire `ThemeRegistry` into `frontend/src/app/layout.tsx`**

```tsx
import type { Metadata } from "next";
import ThemeRegistry from "./ThemeRegistry";
import "./globals.css";

export const metadata: Metadata = {
  title: "RustySERRF",
  description: "SERRF normalization for metabolomics data",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>
        <ThemeRegistry>{children}</ThemeRegistry>
      </body>
    </html>
  );
}
```

- [ ] **Step 7: Add `ThemeToggle` next to the title in `frontend/src/app/page.tsx`**

```tsx
import ThemeToggle from "../components/ThemeToggle";

export default function Home() {
  return (
    <main>
      <h1>RustySERRF</h1>
      <ThemeToggle />
    </main>
  );
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `npm test`
Expected: PASS, all tests including `page.test.tsx` (unaffected — it only checks the heading).

- [ ] **Step 9: Run typecheck, lint, and build**

Run: `npm run typecheck && npm run lint && npm run build`
Expected: all three PASS.

- [ ] **Step 10: Commit**

```bash
git add frontend
git commit -m "Add MUI theme with persisted dark/light toggle"
```

---

### Task 5: `lib/types.ts` and `lib/api.ts` — typed fetch/SSE wrappers

**Files:**
- Create: `frontend/src/lib/types.ts`
- Create: `frontend/src/lib/api.ts`
- Create: `frontend/src/lib/api.test.ts`

**Interfaces:**
- Produces:
  - `types.ts`: `JobEvent` (discriminated union on `status`, mirrors `crate::job::JobEvent`'s serde shape from Task 1/existing code), `PcaJson` (`{ pc1: number[]; pc2: number[]; sample_type: (string | null)[]; batch: string[] }`, mirrors Task 2), `ResultJson` (mirrors `crates/serrf-api/src/routes/result.rs`'s `ResultJson`).
  - `api.ts`: `class ApiError extends Error { status: number }`; `uploadDataset(file: File): Promise<{ jobId: string }>`; `subscribeToJobEvents(jobId: string, onEvent: (event: JobEvent) => void): () => void` (returns an unsubscribe function); `fetchJobStatus(jobId: string): Promise<JobEvent>`; `fetchJobResult(jobId: string): Promise<ResultJson>`; `downloadUrl(jobId: string): string`.
- Consumes: nothing from earlier tasks besides the scaffold. The API base is read lazily via `process.env.NEXT_PUBLIC_API_BASE` inside each function (not a module-level constant) specifically so tests can set it after the module is imported — see Task 12, which relies on this.

- [ ] **Step 1: Write the failing unit tests**

Create `frontend/src/lib/api.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, downloadUrl, fetchJobResult, fetchJobStatus, subscribeToJobEvents, uploadDataset } from "./api";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  listeners: Record<string, ((event: MessageEvent) => void)[]> = {};
  closed = false;

  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners[type] = [...(this.listeners[type] ?? []), listener];
  }

  removeEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners[type] = (this.listeners[type] ?? []).filter((l) => l !== listener);
  }

  emit(type: string, data: unknown) {
    for (const listener of this.listeners[type] ?? []) {
      listener({ data: JSON.stringify(data) } as MessageEvent);
    }
  }

  close() {
    this.closed = true;
  }
}

beforeEach(() => {
  FakeEventSource.instances = [];
  vi.stubGlobal("EventSource", FakeEventSource);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("uploadDataset", () => {
  it("posts multipart form data and returns the job id", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ job_id: "abc-123" }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const file = new File(["a,b\n1,2"], "dataset.csv", { type: "text/csv" });
    const result = await uploadDataset(file);

    expect(result).toEqual({ jobId: "abc-123" });
    expect(fetchMock).toHaveBeenCalledWith("/api/jobs", expect.objectContaining({ method: "POST" }));
  });

  it("throws ApiError with the server's message on a non-ok response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 400, json: async () => ({ error: "bad batch" }) })
    );

    const file = new File(["x"], "dataset.csv");
    await expect(uploadDataset(file)).rejects.toMatchObject(new ApiError("bad batch", 400));
  });
});

describe("subscribeToJobEvents", () => {
  it("parses progress events from the SSE stream", () => {
    const events: unknown[] = [];
    const unsubscribe = subscribeToJobEvents("job-1", (event) => events.push(event));

    const source = FakeEventSource.instances[0];
    expect(source.url).toBe("/api/jobs/job-1/events");
    source.emit("progress", { status: "progress", stage: "SERRF normalization", current: 1, total: 10 });

    expect(events).toEqual([{ status: "progress", stage: "SERRF normalization", current: 1, total: 10 }]);

    unsubscribe();
    expect(source.closed).toBe(true);
  });
});

describe("fetchJobStatus and fetchJobResult", () => {
  it("fetchJobStatus GETs the status endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ status: "queued" }) });
    vi.stubGlobal("fetch", fetchMock);

    const result = await fetchJobStatus("job-1");

    expect(fetchMock).toHaveBeenCalledWith("/api/jobs/job-1");
    expect(result).toEqual({ status: "queued" });
  });

  it("fetchJobResult GETs the result endpoint", async () => {
    const resultJson = {
      compound_labels: ["c1"],
      qc_rsd_raw: [0.1],
      qc_rsd_serrf: [0.01],
      validate_rsd_raw: {},
      validate_rsd_serrf: {},
      pca_before: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
      pca_after: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
    };
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => resultJson }));

    const result = await fetchJobResult("job-1");

    expect(result).toEqual(resultJson);
  });
});

describe("downloadUrl", () => {
  it("builds the download path for a job id", () => {
    expect(downloadUrl("job-1")).toBe("/api/jobs/job-1/download");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- api.test`
Expected: FAIL to compile — `./api` doesn't exist yet.

- [ ] **Step 3: Create `frontend/src/lib/types.ts`**

```ts
export type JobEvent =
  | { status: "queued" }
  | { status: "progress"; stage: string; current: number; total: number }
  | { status: "completed" }
  | { status: "failed"; error: string };

export interface PcaJson {
  pc1: number[];
  pc2: number[];
  sample_type: (string | null)[];
  batch: string[];
}

export interface ResultJson {
  compound_labels: string[];
  qc_rsd_raw: number[];
  qc_rsd_serrf: number[];
  validate_rsd_raw: Record<string, number[]>;
  validate_rsd_serrf: Record<string, number[]>;
  pca_before: PcaJson;
  pca_after: PcaJson;
}
```

- [ ] **Step 4: Create `frontend/src/lib/api.ts`**

```ts
import type { JobEvent, ResultJson } from "./types";

function apiBase(): string {
  return process.env.NEXT_PUBLIC_API_BASE ?? "";
}

export class ApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

async function parseErrorMessage(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: string };
    return typeof body.error === "string" ? body.error : response.statusText;
  } catch {
    return response.statusText;
  }
}

export async function uploadDataset(file: File): Promise<{ jobId: string }> {
  const form = new FormData();
  form.append("file", file);
  const response = await fetch(`${apiBase()}/api/jobs`, { method: "POST", body: form });
  if (!response.ok) {
    throw new ApiError(await parseErrorMessage(response), response.status);
  }
  const body = (await response.json()) as { job_id: string };
  return { jobId: body.job_id };
}

export function subscribeToJobEvents(jobId: string, onEvent: (event: JobEvent) => void): () => void {
  const source = new EventSource(`${apiBase()}/api/jobs/${jobId}/events`);
  const statuses: JobEvent["status"][] = ["queued", "progress", "completed", "failed"];
  const registered = statuses.map((status) => {
    const listener = (message: MessageEvent<string>) => {
      onEvent(JSON.parse(message.data) as JobEvent);
    };
    source.addEventListener(status, listener as EventListener);
    return { status, listener };
  });

  return () => {
    registered.forEach(({ status, listener }) => source.removeEventListener(status, listener as EventListener));
    source.close();
  };
}

export async function fetchJobStatus(jobId: string): Promise<JobEvent> {
  const response = await fetch(`${apiBase()}/api/jobs/${jobId}`);
  if (!response.ok) {
    throw new ApiError(await parseErrorMessage(response), response.status);
  }
  return (await response.json()) as JobEvent;
}

export async function fetchJobResult(jobId: string): Promise<ResultJson> {
  const response = await fetch(`${apiBase()}/api/jobs/${jobId}/result`);
  if (!response.ok) {
    throw new ApiError(await parseErrorMessage(response), response.status);
  }
  return (await response.json()) as ResultJson;
}

export function downloadUrl(jobId: string): string {
  return `${apiBase()}/api/jobs/${jobId}/download`;
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npm test -- api.test`
Expected: PASS, 6 tests.

- [ ] **Step 6: Run the full unit suite, typecheck, lint**

Run: `npm test && npm run typecheck && npm run lint`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib
git commit -m "Add typed fetch/SSE API client (lib/api.ts, lib/types.ts)"
```

---

### Task 6: `useJob` hook — the upload/progress/result state machine

**Files:**
- Create: `frontend/src/hooks/useJob.ts`
- Create: `frontend/src/hooks/useJob.test.ts`

**Interfaces:**
- Produces: `export type JobState = { phase: "idle" } | { phase: "uploading" } | { phase: "processing"; jobId: string; stage?: string; current?: number; total?: number } | { phase: "done"; jobId: string; result: ResultJson } | { phase: "error"; message: string }`; `export function useJob(): { state: JobState; submit: (file: File) => void; reset: () => void }`.
- Consumes: `../lib/api`'s `uploadDataset`, `subscribeToJobEvents`, `fetchJobResult` (Task 5) — mocked via `vi.mock("../lib/api")` in this task's tests, since Task 5 already covers `lib/api.ts`'s own correctness against a fake network boundary; testing this hook against the same fake boundary again would just re-test Task 5.

- [ ] **Step 1: Write the failing unit tests**

Create `frontend/src/hooks/useJob.test.ts`:

```ts
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useJob } from "./useJob";
import * as api from "../lib/api";
import type { JobEvent, ResultJson } from "../lib/types";

vi.mock("../lib/api");

const resultFixture: ResultJson = {
  compound_labels: ["c1"],
  qc_rsd_raw: [0.1],
  qc_rsd_serrf: [0.01],
  validate_rsd_raw: {},
  validate_rsd_serrf: {},
  pca_before: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
  pca_after: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
};

afterEach(() => {
  vi.resetAllMocks();
});

describe("useJob", () => {
  it("moves idle -> processing -> done as events arrive", async () => {
    let emit: (event: JobEvent) => void = () => {};
    vi.mocked(api.uploadDataset).mockResolvedValue({ jobId: "job-1" });
    vi.mocked(api.subscribeToJobEvents).mockImplementation((_jobId, onEvent) => {
      emit = onEvent;
      return () => {};
    });
    vi.mocked(api.fetchJobResult).mockResolvedValue(resultFixture);

    const { result } = renderHook(() => useJob());
    expect(result.current.state).toEqual({ phase: "idle" });

    await act(async () => {
      result.current.submit(new File(["x"], "dataset.csv"));
    });
    await waitFor(() => expect(result.current.state.phase).toBe("processing"));

    act(() => emit({ status: "progress", stage: "SERRF normalization", current: 3, total: 10 }));
    expect(result.current.state).toEqual({
      phase: "processing",
      jobId: "job-1",
      stage: "SERRF normalization",
      current: 3,
      total: 10,
    });

    await act(async () => emit({ status: "completed" }));
    await waitFor(() =>
      expect(result.current.state).toEqual({ phase: "done", jobId: "job-1", result: resultFixture })
    );
  });

  it("moves to error when the job fails", async () => {
    let emit: (event: JobEvent) => void = () => {};
    vi.mocked(api.uploadDataset).mockResolvedValue({ jobId: "job-1" });
    vi.mocked(api.subscribeToJobEvents).mockImplementation((_jobId, onEvent) => {
      emit = onEvent;
      return () => {};
    });

    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));
    await waitFor(() => expect(result.current.state.phase).toBe("processing"));

    act(() => emit({ status: "failed", error: "batch B has too few QC" }));

    expect(result.current.state).toEqual({ phase: "error", message: "batch B has too few QC" });
  });

  it("moves to error when the upload itself rejects", async () => {
    vi.mocked(api.uploadDataset).mockRejectedValue(new Error("network down"));

    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));

    await waitFor(() => expect(result.current.state).toEqual({ phase: "error", message: "network down" }));
  });

  it("reset returns to idle from any state", async () => {
    vi.mocked(api.uploadDataset).mockRejectedValue(new Error("boom"));
    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));
    await waitFor(() => expect(result.current.state.phase).toBe("error"));

    act(() => result.current.reset());

    expect(result.current.state).toEqual({ phase: "idle" });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- useJob`
Expected: FAIL to compile — `./useJob` doesn't exist yet.

- [ ] **Step 3: Implement `frontend/src/hooks/useJob.ts`**

```ts
"use client";

import { useCallback, useRef, useState } from "react";
import { fetchJobResult, subscribeToJobEvents, uploadDataset } from "../lib/api";
import type { JobEvent, ResultJson } from "../lib/types";

export type JobState =
  | { phase: "idle" }
  | { phase: "uploading" }
  | { phase: "processing"; jobId: string; stage?: string; current?: number; total?: number }
  | { phase: "done"; jobId: string; result: ResultJson }
  | { phase: "error"; message: string };

export function useJob() {
  const [state, setState] = useState<JobState>({ phase: "idle" });
  const unsubscribeRef = useRef<(() => void) | null>(null);

  const submit = useCallback((file: File) => {
    setState({ phase: "uploading" });
    uploadDataset(file)
      .then(({ jobId }) => {
        setState({ phase: "processing", jobId });
        unsubscribeRef.current = subscribeToJobEvents(jobId, (event: JobEvent) => {
          if (event.status === "progress") {
            setState({ phase: "processing", jobId, stage: event.stage, current: event.current, total: event.total });
          } else if (event.status === "completed") {
            unsubscribeRef.current?.();
            fetchJobResult(jobId)
              .then((result) => setState({ phase: "done", jobId, result }))
              .catch((error: Error) => setState({ phase: "error", message: error.message }));
          } else if (event.status === "failed") {
            unsubscribeRef.current?.();
            setState({ phase: "error", message: event.error });
          }
        });
      })
      .catch((error: Error) => setState({ phase: "error", message: error.message }));
  }, []);

  const reset = useCallback(() => {
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
    setState({ phase: "idle" });
  }, []);

  return { state, submit, reset };
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- useJob`
Expected: PASS, 4 tests.

- [ ] **Step 5: Run the full unit suite, typecheck, lint**

Run: `npm test && npm run typecheck && npm run lint`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/hooks
git commit -m "Add useJob hook: idle/uploading/processing/done/error state machine"
```

---

### Task 7: `UploadForm` component

**Files:**
- Create: `frontend/src/components/UploadForm.tsx`
- Create: `frontend/src/components/UploadForm.test.tsx`

**Interfaces:**
- Produces: `export default function UploadForm(props: { onSubmit: (file: File) => void; errorMessage?: string }): JSX.Element`.
- Consumes: nothing beyond MUI primitives.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/UploadForm.test.tsx`:

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

  it("shows an error message when provided", () => {
    render(<UploadForm onSubmit={vi.fn()} errorMessage="batch B has too few QC" />);

    expect(screen.getByText("batch B has too few QC")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- UploadForm`
Expected: FAIL to compile — `./UploadForm` doesn't exist yet.

- [ ] **Step 3: Implement `frontend/src/components/UploadForm.tsx`**

```tsx
"use client";

import { useState } from "react";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";

interface UploadFormProps {
  onSubmit: (file: File) => void;
  errorMessage?: string;
}

export default function UploadForm({ onSubmit, errorMessage }: UploadFormProps) {
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
      {errorMessage && (
        <Alert severity="error" sx={{ mt: 2 }}>
          {errorMessage}
        </Alert>
      )}
    </Box>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- UploadForm`
Expected: PASS, 2 tests.

- [ ] **Step 5: Run the full unit suite, typecheck, lint**

Run: `npm test && npm run typecheck && npm run lint`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/UploadForm.tsx frontend/src/components/UploadForm.test.tsx
git commit -m "Add UploadForm component"
```

---

### Task 8: `ProgressView` component

**Files:**
- Create: `frontend/src/components/ProgressView.tsx`
- Create: `frontend/src/components/ProgressView.test.tsx`

**Interfaces:**
- Produces: `export default function ProgressView(props: { stage?: string; current?: number; total?: number }): JSX.Element`.
- Consumes: nothing beyond MUI primitives.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/ProgressView.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import ProgressView from "./ProgressView";

describe("ProgressView", () => {
  it("shows an indeterminate bar and a default message before any progress event", () => {
    render(<ProgressView />);

    expect(screen.getByText(/starting normalization/i)).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
  });

  it("shows a determinate bar and stage/current/total once progress arrives", () => {
    render(<ProgressView stage="SERRF normalization" current={3} total={10} />);

    expect(screen.getByText("SERRF normalization")).toBeInTheDocument();
    expect(screen.getByText("3 / 10")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "30");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- ProgressView`
Expected: FAIL to compile — `./ProgressView` doesn't exist yet.

- [ ] **Step 3: Implement `frontend/src/components/ProgressView.tsx`**

```tsx
import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";
import Typography from "@mui/material/Typography";

interface ProgressViewProps {
  stage?: string;
  current?: number;
  total?: number;
}

export default function ProgressView({ stage, current, total }: ProgressViewProps) {
  const value = current !== undefined && total ? Math.min(100, (current / total) * 100) : undefined;

  return (
    <Box>
      <Typography variant="h6" gutterBottom>
        {stage ?? "Starting normalization…"}
      </Typography>
      {value === undefined ? <LinearProgress /> : <LinearProgress variant="determinate" value={value} />}
      {current !== undefined && total !== undefined && (
        <Typography variant="body2" sx={{ mt: 1 }}>
          {current} / {total}
        </Typography>
      )}
    </Box>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- ProgressView`
Expected: PASS, 2 tests.

- [ ] **Step 5: Run the full unit suite, typecheck, lint**

Run: `npm test && npm run typecheck && npm run lint`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/ProgressView.tsx frontend/src/components/ProgressView.test.tsx
git commit -m "Add ProgressView component"
```

---

### Task 9: `RsdBarChart` component

**Files:**
- Create: `frontend/src/components/RsdBarChart.tsx`
- Create: `frontend/src/components/RsdBarChart.test.tsx`

**Interfaces:**
- Produces: `export default function RsdBarChart(props: { compoundLabels: string[]; qcRsdRaw: number[]; qcRsdSerrf: number[] }): JSX.Element`.
- Consumes: `@mui/x-charts/BarChart`.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/RsdBarChart.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import RsdBarChart from "./RsdBarChart";

describe("RsdBarChart", () => {
  it("renders a chart with both series labeled", () => {
    const { container } = render(
      <RsdBarChart compoundLabels={["c1", "c2"]} qcRsdRaw={[0.2, 0.3]} qcRsdSerrf={[0.05, 0.06]} />
    );

    expect(container.querySelector("svg")).toBeInTheDocument();
    expect(screen.getByText("Raw QC-RSD")).toBeInTheDocument();
    expect(screen.getByText("SERRF QC-RSD")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- RsdBarChart`
Expected: FAIL to compile — `./RsdBarChart` doesn't exist yet.

- [ ] **Step 3: Implement `frontend/src/components/RsdBarChart.tsx`**

```tsx
import { BarChart } from "@mui/x-charts/BarChart";

interface RsdBarChartProps {
  compoundLabels: string[];
  qcRsdRaw: number[];
  qcRsdSerrf: number[];
}

export default function RsdBarChart({ compoundLabels, qcRsdRaw, qcRsdSerrf }: RsdBarChartProps) {
  return (
    <BarChart
      height={400}
      xAxis={[{ scaleType: "band", data: compoundLabels, label: "Compound" }]}
      yAxis={[{ label: "QC-RSD" }]}
      series={[
        { data: qcRsdRaw, label: "Raw QC-RSD" },
        { data: qcRsdSerrf, label: "SERRF QC-RSD" },
      ]}
    />
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- RsdBarChart`
Expected: PASS, 1 test.

- [ ] **Step 5: Run the full unit suite, typecheck, lint**

Run: `npm test && npm run typecheck && npm run lint`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/RsdBarChart.tsx frontend/src/components/RsdBarChart.test.tsx
git commit -m "Add RsdBarChart component"
```

---

### Task 10: `PcaScatter` component

**Files:**
- Create: `frontend/src/components/PcaScatter.tsx`
- Create: `frontend/src/components/PcaScatter.test.tsx`

**Interfaces:**
- Produces: `export default function PcaScatter(props: { pc1: number[]; pc2: number[]; sampleType: (string | null)[]; title: string }): JSX.Element` — groups points into one chart series per distinct `sampleType` value (`null` grouped under `"unknown"`), so the legend doubles as the color key.
- Consumes: `@mui/x-charts/ScatterChart`.

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/PcaScatter.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import PcaScatter from "./PcaScatter";

describe("PcaScatter", () => {
  it("renders a titled chart with one legend entry per sample type", () => {
    const { container } = render(
      <PcaScatter
        title="Before normalization"
        pc1={[1, 2, 3]}
        pc2={[4, 5, 6]}
        sampleType={["qc", "sample", null]}
      />
    );

    expect(screen.getByText("Before normalization")).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeInTheDocument();
    expect(screen.getByText("qc")).toBeInTheDocument();
    expect(screen.getByText("sample")).toBeInTheDocument();
    expect(screen.getByText("unknown")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- PcaScatter`
Expected: FAIL to compile — `./PcaScatter` doesn't exist yet.

- [ ] **Step 3: Implement `frontend/src/components/PcaScatter.tsx`**

```tsx
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { ScatterChart } from "@mui/x-charts/ScatterChart";

interface PcaScatterProps {
  pc1: number[];
  pc2: number[];
  sampleType: (string | null)[];
  title: string;
}

export default function PcaScatter({ pc1, pc2, sampleType, title }: PcaScatterProps) {
  const groups = Array.from(new Set(sampleType.map((type) => type ?? "unknown")));
  const series = groups.map((group) => {
    const indices = sampleType
      .map((type, index) => ((type ?? "unknown") === group ? index : -1))
      .filter((index) => index !== -1);
    return {
      label: group,
      data: indices.map((index) => ({ id: index, x: pc1[index], y: pc2[index] })),
    };
  });

  return (
    <Box>
      <Typography variant="subtitle1" gutterBottom>
        {title}
      </Typography>
      <ScatterChart height={400} series={series} xAxis={[{ label: "PC1" }]} yAxis={[{ label: "PC2" }]} />
    </Box>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- PcaScatter`
Expected: PASS, 1 test.

- [ ] **Step 5: Run the full unit suite, typecheck, lint**

Run: `npm test && npm run typecheck && npm run lint`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/PcaScatter.tsx frontend/src/components/PcaScatter.test.tsx
git commit -m "Add PcaScatter component"
```

---

### Task 11: `ResultsView` component

**Files:**
- Create: `frontend/src/components/ResultsView.tsx`
- Create: `frontend/src/components/ResultsView.test.tsx`

**Interfaces:**
- Produces: `export default function ResultsView(props: { jobId: string; result: ResultJson; onReset: () => void }): JSX.Element` — composes `RsdBarChart` (Task 9) and two `PcaScatter`s (Task 10, "Before normalization"/"After normalization"), a one-line summary (compound count + median RSD raw vs. SERRF), a download link (`downloadUrl` from `lib/api`, Task 5), and a "start a new run" button calling `onReset`.
- Consumes: `RsdBarChart` (Task 9), `PcaScatter` (Task 10), `downloadUrl` (Task 5), `ResultJson` (Task 5).

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/ResultsView.test.tsx`:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ResultsView from "./ResultsView";
import type { ResultJson } from "../lib/types";

const result: ResultJson = {
  compound_labels: ["c1", "c2"],
  qc_rsd_raw: [0.2, 0.4],
  qc_rsd_serrf: [0.02, 0.04],
  validate_rsd_raw: {},
  validate_rsd_serrf: {},
  pca_before: { pc1: [1, 2], pc2: [3, 4], sample_type: ["qc", "sample"], batch: ["A", "A"] },
  pca_after: { pc1: [1, 2], pc2: [3, 4], sample_type: ["qc", "sample"], batch: ["A", "A"] },
};

describe("ResultsView", () => {
  it("shows a summary, both PCA panels, and a working download link and reset button", async () => {
    const onReset = vi.fn();
    render(<ResultsView jobId="job-1" result={result} onReset={onReset} />);

    expect(screen.getByText(/2 compounds/i)).toBeInTheDocument();
    expect(screen.getByText("Before normalization")).toBeInTheDocument();
    expect(screen.getByText("After normalization")).toBeInTheDocument();

    const downloadLink = screen.getByRole("link", { name: /download results/i });
    expect(downloadLink).toHaveAttribute("href", "/api/jobs/job-1/download");

    await userEvent.click(screen.getByRole("button", { name: /start a new run/i }));
    expect(onReset).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- ResultsView`
Expected: FAIL to compile — `./ResultsView` doesn't exist yet.

- [ ] **Step 3: Implement `frontend/src/components/ResultsView.tsx`**

```tsx
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import Typography from "@mui/material/Typography";
import { downloadUrl } from "../lib/api";
import type { ResultJson } from "../lib/types";
import RsdBarChart from "./RsdBarChart";
import PcaScatter from "./PcaScatter";

interface ResultsViewProps {
  jobId: string;
  result: ResultJson;
  onReset: () => void;
}

function median(values: number[]): number {
  const sorted = [...values].filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  return sorted.length === 0 ? 0 : sorted[Math.floor(sorted.length / 2)];
}

export default function ResultsView({ jobId, result, onReset }: ResultsViewProps) {
  return (
    <Box>
      <Typography variant="h5" gutterBottom>
        Results
      </Typography>
      <Typography variant="body1" sx={{ mb: 2 }}>
        {result.compound_labels.length} compounds — median QC-RSD raw {median(result.qc_rsd_raw).toFixed(3)}, SERRF{" "}
        {median(result.qc_rsd_serrf).toFixed(3)}
      </Typography>
      <Grid container spacing={4}>
        <Grid item xs={12}>
          <RsdBarChart
            compoundLabels={result.compound_labels}
            qcRsdRaw={result.qc_rsd_raw}
            qcRsdSerrf={result.qc_rsd_serrf}
          />
        </Grid>
        <Grid item xs={12} md={6}>
          <PcaScatter
            title="Before normalization"
            pc1={result.pca_before.pc1}
            pc2={result.pca_before.pc2}
            sampleType={result.pca_before.sample_type}
          />
        </Grid>
        <Grid item xs={12} md={6}>
          <PcaScatter
            title="After normalization"
            pc1={result.pca_after.pc1}
            pc2={result.pca_after.pc2}
            sampleType={result.pca_after.sample_type}
          />
        </Grid>
      </Grid>
      <Box sx={{ mt: 3, display: "flex", gap: 2 }}>
        <Button variant="contained" href={downloadUrl(jobId)}>
          Download results (.zip)
        </Button>
        <Button variant="outlined" onClick={onReset}>
          Start a new run
        </Button>
      </Box>
    </Box>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- ResultsView`
Expected: PASS, 1 test.

- [ ] **Step 5: Run the full unit suite, typecheck, lint, and the coverage gate**

Run: `npm test && npm run typecheck && npm run lint && npm run test:coverage`
Expected: all PASS; `test:coverage` should already be comfortably above 80% given every component/hook/lib file has direct tests — if any metric is below 80%, note the gap here rather than in Task 15 and add the missing test now.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/ResultsView.tsx frontend/src/components/ResultsView.test.tsx
git commit -m "Add ResultsView component"
```

---

### Task 12: Wire `page.tsx` end-to-end + real-backend integration test

**Files:**
- Modify: `frontend/src/app/page.tsx`
- Modify: `frontend/src/app/page.test.tsx`
- Create: `frontend/tests/fixtures/example-dataset.csv`
- Create: `frontend/tests/integration/full-flow.test.tsx`

**Interfaces:**
- Produces: `page.tsx` renders `UploadForm` (idle), `ProgressView` (uploading/processing), `ResultsView` (done), or an error `Alert` + "start over" button (error), driven entirely by `useJob()`'s `state`/`submit`/`reset`.
- Consumes: `useJob` (Task 6), `UploadForm` (Task 7), `ProgressView` (Task 8), `ResultsView` (Task 11), `ThemeToggle` (Task 4).

- [ ] **Step 1: Create the shared fixture dataset**

Create `frontend/tests/fixtures/example-dataset.csv` (a small valid transposed-layout dataset: 2 batches, 6 QC + 4 samples each, 3 compounds — matches the shape used in `serrf-api`'s own Rust tests, so it validates and normalizes in well under a second):

```csv
,batch,A,A,B,B,A,A,B,B,A,A,B,B,A,A,B,B,A,A,B,B
,sampleType,qc,qc,qc,qc,qc,qc,qc,qc,qc,qc,qc,qc,sample,sample,sample,sample,sample,sample,sample,sample
,time,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19
No,label,s0,s1,s2,s3,s4,s5,s6,s7,s8,s9,s10,s11,s12,s13,s14,s15,s16,s17,s18,s19
1,Compound0,100,101,102,100,101,102,100,101,102,100,101,102,100,101,102,100,101,102,100,101
2,Compound1,101,102,103,101,102,103,101,102,103,101,102,103,101,102,103,101,102,103,101,102
3,Compound2,102,103,104,102,103,104,102,103,104,102,103,104,102,103,104,102,103,104,102,103
```

- [ ] **Step 2: Write the failing test for the wired page**

Replace `frontend/src/app/page.test.tsx`:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import Home from "./page";

vi.mock("../lib/api");

describe("Home", () => {
  it("renders the app title and starts in the upload state", () => {
    render(<Home />);

    expect(screen.getByRole("heading", { name: "RustySERRF" })).toBeInTheDocument();
    expect(screen.getByText(/upload a dataset/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `npm test -- app/page.test`
Expected: FAIL — current `page.tsx` doesn't render `UploadForm`, so "upload a dataset" isn't found.

- [ ] **Step 4: Implement the wired `frontend/src/app/page.tsx`**

```tsx
"use client";

import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Container from "@mui/material/Container";
import { useJob } from "../hooks/useJob";
import ThemeToggle from "../components/ThemeToggle";
import UploadForm from "../components/UploadForm";
import ProgressView from "../components/ProgressView";
import ResultsView from "../components/ResultsView";

export default function Home() {
  const { state, submit, reset } = useJob();

  return (
    <Container maxWidth="lg" sx={{ py: 4 }}>
      <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 3 }}>
        <h1>RustySERRF</h1>
        <ThemeToggle />
      </Box>

      {state.phase === "idle" && <UploadForm onSubmit={submit} />}
      {state.phase === "uploading" && <ProgressView />}
      {state.phase === "processing" && (
        <ProgressView stage={state.stage} current={state.current} total={state.total} />
      )}
      {state.phase === "done" && <ResultsView jobId={state.jobId} result={state.result} onReset={reset} />}
      {state.phase === "error" && (
        <Box>
          <Alert severity="error" sx={{ mb: 2 }}>
            {state.message}
          </Alert>
          <Button variant="outlined" onClick={reset}>
            Start over
          </Button>
        </Box>
      )}
    </Container>
  );
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `npm test -- app/page.test`
Expected: PASS, 1 test.

- [ ] **Step 6: Write the real-backend integration test**

Create `frontend/tests/integration/full-flow.test.tsx` (this is the layer honoring "real deps only" — it spawns the actual `serrf-api` binary and drives the real page against it, no fetch/EventSource mocking):

```tsx
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import Home from "../../src/app/page";

let apiProcess: ChildProcessWithoutNullStreams;

beforeAll(async () => {
  const apiBase = await new Promise<string>((resolve, reject) => {
    apiProcess = spawn("cargo", ["run", "-p", "serrf-api"], {
      cwd: path.resolve(__dirname, "../../.."),
      env: { ...process.env, PORT: "0" },
    });
    let settled = false;
    apiProcess.stdout.on("data", (chunk: Buffer) => {
      const match = chunk.toString().match(/listening on (\S+)/);
      if (match && !settled) {
        settled = true;
        resolve(`http://${match[1].replace("0.0.0.0", "127.0.0.1")}`);
      }
    });
    apiProcess.on("error", reject);
    apiProcess.on("exit", (code) => {
      if (!settled) reject(new Error(`serrf-api exited early with code ${code}`));
    });
  });
  process.env.NEXT_PUBLIC_API_BASE = apiBase;
}, 300_000);

afterAll(() => {
  apiProcess?.kill();
  delete process.env.NEXT_PUBLIC_API_BASE;
});

describe("full upload-to-results flow against a real serrf-api", () => {
  it("uploads a dataset, watches progress, and renders downloadable results", async () => {
    render(<Home />);

    const csvContent = readFileSync(path.resolve(__dirname, "../fixtures/example-dataset.csv"), "utf-8");
    const file = new File([csvContent], "dataset.csv", { type: "text/csv" });
    await userEvent.upload(screen.getByLabelText(/dataset file/i), file);
    await userEvent.click(screen.getByRole("button", { name: /run serrf normalization/i }));

    await waitFor(() => expect(screen.getByText("Results")).toBeInTheDocument(), { timeout: 60_000 });

    const downloadLink = screen.getByRole("link", { name: /download results/i });
    expect(downloadLink.getAttribute("href")).toMatch(/\/api\/jobs\/.+\/download/);
  }, 65_000);
});
```

- [ ] **Step 7: Run the integration test to verify it passes**

Run: `npm run test:integration`
Expected: PASS, 1 test (allow extra time on first run if the Rust workspace isn't already built — run `cargo build -p serrf-api` beforehand to warm the build cache and keep the 300s `beforeAll` timeout comfortable).

- [ ] **Step 8: Run the full unit suite, typecheck, lint, and coverage gate once more**

Run: `npm test && npm run typecheck && npm run lint && npm run test:coverage`
Expected: all PASS, 80%+ on all four coverage metrics for `src/`.

- [ ] **Step 9: Commit**

```bash
git add frontend/src/app/page.tsx frontend/src/app/page.test.tsx frontend/tests
git commit -m "Wire page.tsx to the full job state machine; add real-backend integration test"
```

---

### Task 13: `dev-start.sh` / `dev-stop.sh`

**Files:**
- Create: `dev-start.sh` (repo root)
- Create: `dev-stop.sh` (repo root)
- Create: `smoke-test.sh` (repo root)

**Interfaces:**
- Produces: `./dev-start.sh` builds and starts `serrf-api` (debug build, fast iteration) and the Next.js dev server, recording both PIDs in a gitignored `.dev.pid`; idempotent (a second run without stopping first reports what's already running and exits non-zero rather than double-starting). `./dev-stop.sh` reads `.dev.pid`, kills both processes, removes the file. `./smoke-test.sh` boots the stack via `dev-start.sh`, curls `serrf-api`'s `/health` and the frontend's `/`, then tears down via `dev-stop.sh` regardless of outcome — the scripted smoke check the design spec calls for.
- Consumes: nothing from earlier tasks besides the finished `frontend/` app and `serrf-api` binary target.

- [ ] **Step 1: Add `.dev.pid` to the root `.gitignore`**

Check the repo root `.gitignore` for a `.dev.pid` entry; if absent, append it:

```
.dev.pid
```

- [ ] **Step 2: Create `dev-start.sh`**

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ -f .dev.pid ]; then
  echo "Already running (see .dev.pid). Run ./dev-stop.sh first." >&2
  exit 1
fi

echo "Starting serrf-api..."
cargo run -p serrf-api > /tmp/serrf-api.log 2>&1 &
API_PID=$!

echo "Starting Next.js dev server..."
(cd frontend && npm run dev) > /tmp/serrf-frontend.log 2>&1 &
FRONTEND_PID=$!

echo "$API_PID $FRONTEND_PID" > .dev.pid
echo "serrf-api (pid $API_PID) logging to /tmp/serrf-api.log"
echo "frontend (pid $FRONTEND_PID) logging to /tmp/serrf-frontend.log"
echo "Run ./dev-stop.sh to stop both."
```

- [ ] **Step 3: Create `dev-stop.sh`**

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ ! -f .dev.pid ]; then
  echo "Nothing to stop (.dev.pid not found)." >&2
  exit 1
fi

read -r API_PID FRONTEND_PID < .dev.pid
kill "$API_PID" 2>/dev/null || true
kill "$FRONTEND_PID" 2>/dev/null || true
rm .dev.pid
echo "Stopped serrf-api (pid $API_PID) and frontend (pid $FRONTEND_PID)."
```

- [ ] **Step 4: Create `smoke-test.sh`**

This is the scripted smoke check the design spec calls for: boot the stack, confirm both processes actually answer, tear down — distinct from the Playwright E2E (Task 14), which exercises a full normalization run instead of just liveness.

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cleanup() {
  ./dev-stop.sh || true
}
trap cleanup EXIT

./dev-start.sh

echo "Waiting for serrf-api..."
for _ in $(seq 1 30); do
  if curl -sf http://127.0.0.1:8080/health > /dev/null; then
    break
  fi
  sleep 1
done
curl -sf http://127.0.0.1:8080/health | grep -q "^ok$" || { echo "serrf-api /health check failed" >&2; exit 1; }

echo "Waiting for frontend..."
for _ in $(seq 1 60); do
  if curl -sf -o /dev/null http://127.0.0.1:3000; then
    break
  fi
  sleep 1
done
curl -sf -o /dev/null http://127.0.0.1:3000 || { echo "frontend / check failed" >&2; exit 1; }

echo "Smoke test passed: both processes are up and answering."
```

- [ ] **Step 5: Make all three scripts executable**

Run: `chmod +x dev-start.sh dev-stop.sh smoke-test.sh`

- [ ] **Step 6: Verify manually**

Run: `./smoke-test.sh`
Expected: prints "Smoke test passed" and exits 0; `.dev.pid` is gone afterward (the `trap cleanup EXIT` ran `dev-stop.sh`). Then separately run `./dev-start.sh` twice in a row and confirm the second invocation exits 1 with the "already running" message, then `./dev-stop.sh` to clean up.

- [ ] **Step 7: Commit**

```bash
git add dev-start.sh dev-stop.sh smoke-test.sh .gitignore
git commit -m "Add dev-start.sh/dev-stop.sh and a scripted smoke test for the combined dev loop"
```

---

### Task 14: Playwright E2E setup

**Files:**
- Create: `frontend/playwright.config.ts`
- Create: `frontend/e2e/full-run.spec.ts`

**Interfaces:**
- Produces: `npm run e2e` boots a real `serrf-api` (release build, port 8080) and a real production Next.js server (`next build && next start`, port 3000) via Playwright's `webServer` array config, runs `e2e/full-run.spec.ts` against them in a real Chromium browser, then tears both down.
- Consumes: the fixture at `frontend/tests/fixtures/example-dataset.csv` (Task 12), the finished `page.tsx` (Task 12).

- [ ] **Step 1: Create `frontend/playwright.config.ts`**

```ts
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 120_000,
  use: { baseURL: "http://localhost:3000" },
  webServer: [
    {
      command: "cargo run --release -p serrf-api",
      cwd: "..",
      port: 8080,
      timeout: 300_000,
      reuseExistingServer: !process.env.CI,
    },
    {
      command: "npm run build && npm run start",
      port: 3000,
      timeout: 180_000,
      reuseExistingServer: !process.env.CI,
    },
  ],
});
```

- [ ] **Step 2: Create `frontend/e2e/full-run.spec.ts`**

```ts
import { test, expect } from "@playwright/test";
import path from "node:path";

test("upload a dataset, watch progress, view results, download, and toggle theme", async ({ page }) => {
  await page.goto("/");

  await page
    .getByLabel("dataset file")
    .setInputFiles(path.resolve(__dirname, "../tests/fixtures/example-dataset.csv"));
  await page.getByRole("button", { name: "Run SERRF normalization" }).click();

  await expect(page.getByText("Results")).toBeVisible({ timeout: 300_000 });
  await expect(page.locator("svg").first()).toBeVisible();

  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("link", { name: /download results/i }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("serrf-results.zip");

  const toggle = page.getByRole("button", { name: /toggle theme/i });
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  const persisted = await page.evaluate(() => localStorage.getItem("color-mode"));
  expect(persisted).toBe("dark");
});
```

- [ ] **Step 3: Install Playwright's browser binary**

Run: `cd frontend && npx playwright install --with-deps chromium`

- [ ] **Step 4: Run the E2E test**

Run: `npm run e2e`
Expected: PASS, 1 test. First run is slow (release `cargo build` + `next build`) — that's expected; Playwright's `reuseExistingServer` (true outside CI) means subsequent local runs reuse already-running dev servers if you start them yourself instead.

- [ ] **Step 5: Commit**

```bash
git add frontend/playwright.config.ts frontend/e2e
git commit -m "Add Playwright E2E: full upload-to-download-to-theme-toggle run"
```

---

### Task 15: Extend CI for the frontend and E2E

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: two new CI jobs — `frontend` (lint, typecheck, unit tests with coverage gate, build) and `e2e` (builds `serrf-api` release + runs Playwright) — alongside the existing `ci` (Rust) job.
- Consumes: `frontend/package.json`'s `lint`/`typecheck`/`test:coverage`/`build`/`e2e` scripts (Tasks 3-14).

- [ ] **Step 1: Add the `frontend` and `e2e` jobs to `.github/workflows/ci.yml`**

Append after the existing `ci` job (keep the existing `ci` job's Rust steps exactly as they are):

```yaml
  frontend:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: frontend
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
          cache-dependency-path: frontend/package-lock.json

      - run: npm ci
      - run: npm run lint
      - run: npm run typecheck
      - run: npm run test:coverage
      - run: npm run build

  e2e:
    runs-on: ubuntu-latest
    needs: [ci, frontend]
    steps:
      - uses: actions/checkout@v4

      - name: Install system dependencies
        run: sudo apt-get update && sudo apt-get install -y libfontconfig1-dev

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - run: cargo build --release -p serrf-api

      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
          cache-dependency-path: frontend/package-lock.json

      - working-directory: frontend
        run: npm ci

      - working-directory: frontend
        run: npx playwright install --with-deps chromium

      - working-directory: frontend
        run: npm run e2e
```

- [ ] **Step 2: Verify locally as much as CI would**

Run (from `frontend/`): `npm ci && npm run lint && npm run typecheck && npm run test:coverage && npm run build`
Expected: all PASS — this is exactly what the `frontend` CI job runs.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "Add frontend lint/typecheck/test/build and E2E jobs to CI"
```

- [ ] **Step 4: Push the branch and open a PR, following the repo's standard ship flow**

Run: `git push -u origin feat/serrf-frontend`
Then open a PR against `master` (`gh pr create`), wait for all three CI jobs (`ci`, `frontend`, `e2e`) to go green, fixing forward on this branch if any fail — same pattern as Plans 1 and 2. Do not merge without the user's explicit go-ahead on this specific PR.
