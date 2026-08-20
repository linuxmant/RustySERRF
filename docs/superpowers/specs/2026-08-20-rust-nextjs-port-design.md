# SERRF Port: R/Shiny → Rust + Next.js/MUI — Design

## Context

The current app (`app.R`, ~1269 lines) is a single-file Shiny application implementing SERRF
(Systematic Error Removal using Random Forest) normalization for metabolomics data. It depends on
`ranger` (random forest), `data.table`, `openxlsx`/`readxl`, `bootstrap`, and `parallel`. It is slow
and hard to maintain/extend.

Decision: port to a Rust backend + Next.js/MUI frontend, deployable via Docker to any host.
Rust was chosen over Go because `linfa`/`polars`/`ndarray` are closer analogues to `ranger`/
`data.table` than anything in Go's ecosystem — though as detailed below, no existing crate matches
`ranger` closely enough, so the random forest itself will be a custom implementation.

Repo: forked from `slfan2013/Shiny-SERRF` to `linuxmant/RustySERRF` (this fork is `origin`,
upstream is pull-only). Baseline commit already pushed to `master`.

## Goal

**Full parity milestone**: the Rust/Next.js app replicates the entire current pipeline — upload,
validation, SERRF normalization (RF-based, per-compound per-batch), 5-fold cross-validation for QC
RSD estimation, before/after PCA + QC RSD bar plots, and CSV+zip export — before any new features
are considered.

## What the current app actually does (reference, from `app.R`)

- **Input format**: xlsx or csv in a specific transposed layout — sample metadata (`sampleType`,
  `time`, `batch`, `label`) as a header block, compound labels down the first column(s), values in
  the remaining grid. Layout is auto-detected via "first non-NA cell" scanning, not fixed
  coordinates.
- **Validation**: `sampleType` present and contains at least `qc` and `sample`; `time` present and
  unique; `batch` present; every batch has ≥6 QC samples. Missing values → replaced with half the
  row-min. Zeros are preserved as zeros in output. Rows that are all-`Inf` are set aside and
  restored unmodified after normalization.
- **Core algorithm (`serrfR`)**: for each compound `j`, for each batch `b`: fix zeros/NAs in that
  batch's slice with small random jitter; compute spearman correlation matrices among QC and among
  target samples; pick the top-`num` (10) compounds correlated in *both* QC and target sets as
  predictors; scale/train a `ranger` regression model (QC as train, target as test) predicting
  compound `j`'s value from those predictors; predict, rescale by ratios of medians/means back to
  the original scale; detect and patch remaining outliers via `boxplot.stats`; rescale QC and
  non-QC groups to match original batch medians.
- **Cross-validation**: 5-fold CV over QC samples only (stratified so every fold sees every batch),
  each fold re-running `serrfR` with the held-out QC re-labeled as "target", to get an
  out-of-sample QC RSD estimate — this is the reported "SERRF RSD" metric, distinct from the
  RSD of the final full-data normalization.
- **Validate-type samples**: any `sampleType` value other than `qc`/`sample` (e.g. `validate`) is
  normalized the same way (QC as train, that group as target) and reported separately.
- **Output**: `normalized by - none.csv` (raw) and `normalized by - SERRF.csv`, `QC-RSDs.csv`
  (median RSD per method per compound, plus per-validate-type RSDs), a PNG with a QC-RSD barplot
  and before/after PCA scatterplots, all zipped for download.
- **Special case**: if the uploaded data exactly matches the bundled example dataset's shape, the
  app skips computation and serves precomputed reference CSVs instead (a demo fast-path). **Not**
  carried over to the port — it exists only to make the live Shiny demo responsive; the Rust
  version will be fast enough not to need it.

## Architecture

```
RustySERRF/
├── crates/
│   ├── serrf-core/    # pure algorithm lib: parsing, SERRF/RF, RSD, PCA — no HTTP/job deps
│   ├── serrf-api/      # axum HTTP server: upload, job orchestration, SSE progress, results/zip
│   └── serrf-cli/      # thin binary wrapping serrf-core, for local runs + golden-file generation
├── frontend/           # Next.js + MUI app (dark+light mode from day one)
├── golden/             # reference datasets + R-generated expected outputs (checked in)
├── Dockerfile          # single image: Rust release binary + Next.js, both processes in one container
└── dev-start.sh / dev-stop.sh
```

`serrf-core` exposes a pure function, e.g. `normalize(dataset: Dataset, config: SerrfConfig, progress: impl FnMut(Progress)) -> Result<SerrfOutput, SerrfError>`, with no knowledge of HTTP or job
lifecycle. `serrf-api` wraps it in an in-memory job manager (`HashMap<JobId, JobState>`), spawning
each job as a background tokio task and streaming progress over SSE. No database — job state is
ephemeral; acceptable to lose on restart for a single-host research tool.

**Why this split**: keeps the numerically-sensitive algorithm unit-testable in isolation against
golden files, without needing a running server; `serrf-cli` gives a fast local loop for golden-file
generation/validation independent of the frontend.

## Random forest: no drop-in crate — custom implementation, statistical-equivalence validation

`ranger` and any Rust RF crate will not produce bit-identical trees — different RNG streams,
tie-breaking, and split-search implementations mean matching R's output within a tight per-cell
numerical tolerance is not realistically achievable, even with the same seed.

**Decision**: implement a from-scratch CART/bagging regressor in Rust mirroring `ranger`'s defaults
for regression (500 trees, `mtry = max(floor(p/3), 1)`, variance-reduction splitting, bootstrap
sampling), living in `serrf-core`. Validate it against R output using **statistical equivalence**
for RF-dependent outputs (median QC RSD within a tolerance band across the golden datasets; Pearson
correlation between Rust-normalized and R-normalized value vectors above a threshold, e.g. 0.95) —
not exact per-cell match. Deterministic non-RF steps (parsing, RSD calculation, PCA, aggregation,
outlier detection via `boxplot.stats`-equivalent) are held to tight numerical tolerance since they
involve no RNG.

## Data flow

1. **Upload** — `POST /api/jobs` (multipart) → `serrf-core::parse::read_data` (xlsx via `calamine`,
   csv via the `csv` crate) replicates the transposed-layout auto-detection → runs the same
   validation checks as `app.R` → `400` with structured errors on failure, or `202 {job_id}` and
   the job starts.
2. **Processing** — background task: raw RSD → SERRF normalization (custom RF regressor, per
   compound per batch, replicating `serrfR`'s corr-based variable selection + train/predict/
   rescale/outlier-fix logic) → 5-fold CV for QC RSD → validate-type handling if present → PCA
   before/after (via `nalgebra` or `ndarray-linalg` eigendecomposition) → RSD summary stats.
   Progress events (`compound j/n`, stage name) pushed on a channel, matching the granularity of
   the R `incProgress` calls.
3. **Progress** — frontend opens `GET /api/jobs/:id/events` (SSE), renders progress bar + stage
   text.
4. **Results** — `GET /api/jobs/:id/result` (JSON: RSD summary per method, PCA coordinates,
   normalized-value preview) drives interactive charts; `GET /api/jobs/:id/download` streams a zip
   (normalized CSV(s), `QC-RSDs.csv`, and a server-rendered PNG via the `plotters` crate
   replicating the barplot+PCA panel) — matching today's download contents.
5. **Per-compound errors** (e.g. an RF training failure) are recorded and surfaced as warnings in
   the job result rather than aborting the whole job, matching the R script's per-fold `tryCatch`
   pattern.

## Testing strategy

Per standing workflow: TDD (failing test first), 4 layers, 80%+ coverage, no mocks/stubs — real
dependencies throughout.

- **Unit (`serrf-core`)**: parsing edge cases (missing `label`, NA handling, duplicate column
  names), RSD/outlier-removal, PCA math, RF regressor's tree-building/bagging in isolation.
  Deterministic pieces get tight-tolerance golden-file assertions against R output; RF/SERRF
  outputs get statistical-equivalence assertions as described above.
- **Integration (`serrf-api`)**: real HTTP requests against a running axum instance, real file
  uploads (bundled example dataset + at least one edge-case dataset: missing validate type,
  NAs/zeros/all-Inf rows), asserting the full job lifecycle (upload → poll/SSE → result →
  download).
- **E2E (Playwright)**: real browser — upload the example dataset via the UI, watch progress
  update, verify charts render, download the zip and verify its contents; covers dark/light theme
  toggle.
- **Smoke**: `dev-start.sh` boots the container; a scripted smoke test hits `/health` and runs the
  example dataset through the full pipeline once.
- **Golden files**: the bundled example dataset plus its already-committed reference outputs
  (`normalized by - SERRF.csv`, `RSDs - with validate.csv`, `comb_p.csv`) serve as the initial
  golden file set. More real datasets can be added later if gaps show up in review.

## Deployment

Single Docker image running both the Rust API (axum) and the Next.js server as two processes
behind one exposed port — simplest path to deploy to any single host, per standing preference.
`dev-start.sh`/`dev-stop.sh` at the repo root, idempotent via `.dev.pid` (gitignored).

## Out of scope for this milestone

- The "is_example" demo fast-path (precomputed CSVs) — not needed once the real pipeline is fast.
- Auth/multi-user, persistent job history, horizontal scaling / job queue infra (Postgres/Redis) —
  this is a single-host research tool; can be revisited if usage patterns demand it later.
- Matching `ranger`'s RF output bit-for-bit — addressed above via statistical equivalence instead.
