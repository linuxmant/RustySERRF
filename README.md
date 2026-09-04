# RustySERRF

A Rust + Next.js port of SERRF (Systematic Error Removal using Random Forest) normalization for
metabolomics data, deployable via Docker to any host.

This work is based on the original author's R/Shiny implementation:
**[slfan2013/Shiny-SERRF](https://github.com/slfan2013/Shiny-SERRF)**.

## Status

The port is being built in four stages:

1. **`serrf-core` + `serrf-cli`** — done. A pure Rust library implementing the full SERRF
   pipeline (parsing, validation, the random-forest-based normalization core, cross-validation,
   PCA, PNG reporting) plus a CLI binary that runs it end-to-end against a local file. Validated
   against the original R output (statistical equivalence, not bit-for-bit — no two random forest
   implementations produce identical trees).
2. **`serrf-api`** — done. An axum HTTP server wrapping `serrf-core` behind an async job
   API (upload → progress via SSE → JSON results → CSV/PNG zip download).
3. **Frontend** — done. A Next.js/MUI UI replacing the Shiny interface, statically exported
   (`output: "export"`) and served either via `next dev` in development or embedded directly into
   `serrf-api` for the standalone Windows executable (see below).
4. **Docker** — not started. A single-image deployment bundling the API and frontend.

Design details live in [`docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md`](docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md).

## Security posture

`serrf-api` has no authentication and uses a permissive CORS policy
(`CorsLayer::permissive()`). This is deliberate: it's a single-host research tool with no
multi-tenant use case, meant to run behind a reverse proxy (or as the bundled-frontend
standalone executable, which binds to `127.0.0.1` only — see below) rather than exposed
directly to untrusted networks. In the non-bundled deployment mode, `serrf-api` binds
`0.0.0.0` because that's required for Docker's port mapping to reach it; this is not a
security boundary on its own and assumes the container itself is not exposed to an
untrusted network. If this tool is ever deployed multi-tenant or on an untrusted network,
add real authentication first — restricting CORS alone would not meaningfully help. Also
worth knowing: an in-flight normalization job cannot be cancelled once started (it runs on
a background thread pool), so a graceful shutdown signal waits for it to finish, up to a
10-second cap — after that cap, or on a forced kill, the job's results are simply lost.

## Running the Rust CLI

Requires a [Rust toolchain](https://rustup.rs/).

```bash
cargo run -p serrf-cli -- <input-file> --output-dir ./output
```

`<input-file>` is a `.csv` or `.xlsx` file in the [input format](#input-file-format) below (the
same layout the original R app expects). Output is written to `--output-dir` (default
`./output`):

- `normalized-imputed.csv` — the input matrix after missing-value imputation, before SERRF
- `normalized-serrf.csv` — SERRF-normalized values
- `qc-rsds.csv` — per-compound QC RSD, before and after normalization (plus any `validate`-type
  sample groups)
- `report.png` — a QC-RSD bar chart and before/after PCA scatterplot

To run the test suite (includes a slow integration test against the real bundled example
dataset — expect several minutes):

```bash
cargo test --workspace
```

The original R/Shiny app is still live at https://slfan.shinyapps.io/ShinySERRF/ but is no
longer part of this repo.

## Building the standalone Windows executable

`serrf-api` can be built as a single portable `RustySERRF.exe` with the Next.js frontend
embedded, so a non-technical Windows user can double-click it — no Docker, Rust, or Node required
on their machine.

**Prerequisites (on the build machine, not the end user's):**

- Docker
- [`cross`](https://github.com/cross-rs/cross), installed via
  `cargo install cross --git https://github.com/cross-rs/cross`
- Node/npm (for the frontend's static export step)

**Build it:**

```bash
./scripts/build-windows-release.sh
```

This produces `dist/RustySERRF.exe` (~11 MB). `dist/` is gitignored — the `.exe` is a build
artifact, not something committed to the repo.

**A few things worth knowing before handing this to someone:**

1. It must be a `--release` build (already what the script does). `rust-embed` only truly embeds
   assets at compile time in release mode — a debug build instead reads `static-dist/` from disk
   at runtime and would not be portable to another machine.
2. The `.exe` is a console app: the black terminal window that opens *is* the running app.
   Closing that window stops the server.
3. The binary is unsigned, so Windows SmartScreen will show a "Windows protected your PC" warning
   the first time it's run. Code-signing is out of scope for this project.

## Input file format

Follow the [example dataset](https://github.com/slfan2013/Shiny-SERRF/raw/master/SERRF%20example%20dataset.xlsx)
(also bundled in this repo at `golden/example-dataset.xlsx`).

It requires _batch_, _sampleType_, _time_, _label_ for samples, and _No_ for compounds.

_batch_ tells SERRF which samples/qcs belong to one batch, e.g. machine, running period.

_sampleType_ requires _qc_, _sample_. It can also take other validate sample types, e.g.
_validate_.

Note, if you have blank samples which are suggested not to be normalized, leave the cells empty.
Any sample you do not want normalized should be left empty.

_time_ is the processing order. It can be real time values, or simply an integer indicating the
processing order of the samples/qcs.

_label_ is the sample labels (row #4) and compound labels (column B).

_No_ is the compound index.
