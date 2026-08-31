# RustySERRF

A Rust + Next.js port of SERRF (Systematic Error Removal using Random Forest) normalization for
metabolomics data. This repo is being migrated in stages from the original R/Shiny app
(`app.R`, still present in this repo) to a Rust backend + Next.js/MUI frontend, deployable via
Docker to any host.

This work is based on the original author's R/Shiny implementation:
**[slfan2013/Shiny-SERRF](https://github.com/slfan2013/Shiny-SERRF)**.

## Status

The port is being built in four stages:

1. **`serrf-core` + `serrf-cli`** — done. A pure Rust library implementing the full SERRF
   pipeline (parsing, validation, the random-forest-based normalization core, cross-validation,
   PCA, PNG reporting) plus a CLI binary that runs it end-to-end against a local file. Validated
   against the original R output (statistical equivalence, not bit-for-bit — no two random forest
   implementations produce identical trees).
2. **`serrf-api`** — in progress. An axum HTTP server wrapping `serrf-core` behind an async job
   API (upload → progress via SSE → JSON results → CSV/PNG zip download).
3. **Frontend** — not started. A Next.js/MUI UI replacing the Shiny interface.
4. **Docker** — not started. A single-image deployment bundling the API and frontend.

Design details live in [`docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md`](docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md).

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

## Running the original R/Shiny app

The original app (`app.R`) still works and is kept in this repo as the reference implementation
until the Rust port reaches full parity.

**Locally:** open `app.R` in RStudio and click **Run App**, then **Open in Browser**.

**Online:** https://slfan.shinyapps.io/ShinySERRF/

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

## Contact

slfan at ucdavis at edu
