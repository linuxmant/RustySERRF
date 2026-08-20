# SERRF Core Algorithm (serrf-core + serrf-cli) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the SERRF normalization algorithm from `app.R` into a pure, well-tested Rust library (`serrf-core`) plus a thin CLI (`serrf-cli`), with no HTTP/web concerns — this is Plan 1 of 4 (core algorithm → API → frontend → Docker), and produces working, independently-testable software: a command-line tool that normalizes a real dataset end-to-end.

**Architecture:** A Cargo workspace with `crates/serrf-core` (parsing, validation, the custom random-forest regressor, the SERRF algorithm, cross-validation, PCA, PNG reporting — all pure functions, no I/O beyond file parsing) and `crates/serrf-cli` (a binary wrapping `serrf-core`, writing CSV/PNG outputs to disk). Every numerically deterministic step (parsing, RSD, PCA, preprocessing) is golden-file tested against R's actual output with tight tolerance; the random-forest-dependent SERRF output is validated via statistical equivalence (RSD tolerance band + correlation threshold) since no Rust RF implementation will match `ranger`'s RNG/splitting bit-for-bit.

**Tech Stack:** Rust (2021 edition), `ndarray` (matrices), `calamine` (xlsx reading), `csv` (csv reading/writing), `rand`/`rand_chacha` (seeded RNG for the RF), `nalgebra` (SVD for PCA), `plotters` (PNG rendering), `clap` (CLI args), `thiserror` (errors), `assert_cmd`/`tempfile` (test tooling).

**Spec:** `docs/superpowers/specs/2026-08-20-rust-nextjs-port-design.md`

## Global Constraints

- TDD non-negotiable: write the failing test before implementation code, for every task below.
- No mocks/stubs/fakes — tests exercise real parsing, real file I/O (via `tempfile`), real algorithms.
- 80%+ coverage (statements, branches, functions, lines) for `serrf-core` and `serrf-cli` — check with `cargo tarpaulin` (or `cargo llvm-cov` if tarpaulin has issues in this environment) before considering the plan done.
- Commit after every task (not every step) — one focused commit per task, on branch `feat/serrf-core` (create this branch from the tip of `docs/rust-nextjs-port-design` before Task 1).
- RF-dependent outputs are validated via statistical equivalence (RSD tolerance band, correlation threshold), never exact numeric match, per the spec's decision.
- All code lives under `crates/`; do not touch `app.R` or any existing R files — they remain as the reference implementation until this plan and Plan 2 (API) are both merged.

---

### Task 1: Workspace scaffolding + golden fixtures

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/serrf-core/Cargo.toml`, `crates/serrf-core/src/lib.rs`
- Create: `crates/serrf-cli/Cargo.toml`, `crates/serrf-cli/src/main.rs`
- Create: `golden/example-dataset.xlsx` (copy of `SERRF example dataset - with validate.xlsx`)
- Create: `golden/expected/comb_p.csv`, `golden/expected/qc-rsds.csv` (copies of `comb_p.csv`, `RSDs - with validate.csv`)
- Create: `golden/expected/normalized-serrf.csv` (copy of `normalized by - SERRF - with validate.csv`)
- Modify: `.gitignore` (add `/target`)

**Interfaces:**
- Produces: the workspace itself — `serrf-core` and `serrf-cli` crates that later tasks add code to.

- [ ] **Step 1: Create the workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/serrf-core", "crates/serrf-cli"]
```

- [ ] **Step 2: Create `crates/serrf-core/Cargo.toml`**

```toml
[package]
name = "serrf-core"
version = "0.1.0"
edition = "2021"

[dependencies]
ndarray = "0.15"
calamine = "0.24"
csv = "1.3"
rand = "0.8"
rand_chacha = "0.3"
nalgebra = "0.32"
plotters = { version = "0.3", default-features = false, features = ["bitmap_backend", "line_series"] }
thiserror = "1.0"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Create `crates/serrf-core/src/lib.rs`**

```rust
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_version() {
        assert!(!crate_version().is_empty());
    }
}
```

- [ ] **Step 4: Create `crates/serrf-cli/Cargo.toml`**

```toml
[package]
name = "serrf-cli"
version = "0.1.0"
edition = "2021"

[dependencies]
serrf-core = { path = "../serrf-core" }
clap = { version = "4", features = ["derive"] }
anyhow = "1"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 5: Create `crates/serrf-cli/src/main.rs`**

```rust
fn main() {
    println!("serrf-cli {}", serrf_core::crate_version());
}
```

- [ ] **Step 6: Run `cargo build` and `cargo test` from the workspace root, confirm both succeed**

Run: `cargo test`
Expected: `1 passed` (the `reports_a_version` test), workspace builds cleanly.

- [ ] **Step 7: Copy golden fixtures**

```bash
mkdir -p golden/expected
cp "SERRF example dataset - with validate.xlsx" golden/example-dataset.xlsx
cp comb_p.csv golden/expected/comb_p.csv
cp "RSDs - with validate.csv" golden/expected/qc-rsds.csv
cp "normalized by - SERRF - with validate.csv" golden/expected/normalized-serrf.csv
```

- [ ] **Step 8: Add `/target` to `.gitignore`**

- [ ] **Step 9: Commit**

```bash
git checkout -b feat/serrf-core
git add Cargo.toml crates golden .gitignore
git commit -m "Scaffold serrf-core/serrf-cli workspace and golden fixtures"
```

---

### Task 2: Raw CSV grid reader

**Files:**
- Create: `crates/serrf-core/src/parse/mod.rs`
- Create: `crates/serrf-core/src/parse/grid.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod parse;`)

**Interfaces:**
- Produces: `pub fn read_csv_grid(path: &std::path::Path) -> Result<Vec<Vec<Option<String>>>, crate::error::SerrfError>` — a rectangular grid, no header row skipped, empty cells become `None` (mirrors R's `data[data=='']=NA`).
- Consumes: nothing from earlier tasks (first parsing primitive).

- [ ] **Step 1: Write the failing test**

`crates/serrf-core/src/parse/grid.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_a_csv_grid_treating_blanks_as_none() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "a,,c").unwrap();
        writeln!(file, "1,2,").unwrap();
        let grid = read_csv_grid(file.path()).unwrap();
        assert_eq!(
            grid,
            vec![
                vec![Some("a".into()), None, Some("c".into())],
                vec![Some("1".into()), Some("2".into()), None],
            ]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core reads_a_csv_grid`
Expected: FAIL — `read_csv_grid` not found.

- [ ] **Step 3: Implement `read_csv_grid`**

```rust
use crate::error::SerrfError;
use std::path::Path;

pub fn read_csv_grid(path: &Path) -> Result<Vec<Vec<Option<String>>>, SerrfError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)?;
    let mut grid = Vec::new();
    for record in reader.records() {
        let record = record?;
        grid.push(
            record
                .iter()
                .map(|cell| {
                    let trimmed = cell.trim();
                    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
                })
                .collect(),
        );
    }
    Ok(grid)
}

#[cfg(test)]
mod tests {
    // (test from Step 1 goes here)
}
```

Create `crates/serrf-core/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum SerrfError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("xlsx error: {0}")]
    Xlsx(String),
    #[error("could not parse input: {0}")]
    Parse(String),
    #[error("validation failed: {0}")]
    Validation(String),
}
```

Create `crates/serrf-core/src/parse/mod.rs`:

```rust
mod grid;
pub use grid::read_csv_grid;
```

Add to `crates/serrf-core/src/lib.rs`:

```rust
pub mod error;
pub mod parse;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core reads_a_csv_grid`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add raw CSV grid reader"
```

---

### Task 3: Layout detection and table extraction (`grid_to_dataset`)

This is the core of the file-format quirk in `app.R`'s `read_data()`: the file has a "corner" cell
where a vertical column of sample-field-names (e.g. `batch`, `sampleType`, `time`, `label`) and a
horizontal row of compound-field-names (e.g. `No`, `label`) meet. Everything above/left of that
corner is header; below/right is data. The R code finds the corner via "first non-`NA` cell in row
1" (→ column) and "first non-`NA` cell in column 1" (→ row).

**Files:**
- Create: `crates/serrf-core/src/dataset.rs`
- Create: `crates/serrf-core/src/parse/layout.rs`
- Modify: `crates/serrf-core/src/parse/mod.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod dataset;`)

**Interfaces:**
- Consumes: `Vec<Vec<Option<String>>>` grid shape from Task 2.
- Produces:
  - `pub struct RawSampleTable { pub label: Vec<String>, pub columns: std::collections::HashMap<String, Vec<String>> }`
  - `pub struct RawCompoundTable { pub label: Vec<String>, pub columns: std::collections::HashMap<String, Vec<String>> }`
  - `pub struct Dataset { pub samples: RawSampleTable, pub compounds: RawCompoundTable, pub values: ndarray::Array2<f64> }` (rows = compounds, cols = samples; `f64::NAN` = missing)
  - `pub(crate) fn grid_to_dataset(grid: &[Vec<Option<String>>]) -> Result<Dataset, SerrfError>`

- [ ] **Step 1: Write the failing test**

`crates/serrf-core/src/parse/layout.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cell(s: &str) -> Option<String> { Some(s.to_string()) }

    /// A minimal 6x4 grid mirroring the real file format:
    /// row0-2: batch/sampleType/time values per sample (col0 = NA, col1 = corner field name)
    /// row3:   the shared header row (col0="No", col1(corner)="label", col2/3 = sample NAMES)
    /// row4-5: compound rows (col0="No" value, col1="label" value, col2/3 = numeric values)
    fn sample_grid() -> Vec<Vec<Option<String>>> {
        vec![
            vec![None, cell("batch"), cell("A"), cell("B")],
            vec![None, cell("sampleType"), cell("qc"), cell("sample")],
            vec![None, cell("time"), cell("1"), cell("2")],
            vec![cell("No"), cell("label"), cell("S1"), cell("S2")],
            vec![cell("1"), cell("Compound1"), cell("10.5"), cell("20.5")],
            vec![cell("2"), cell("Compound2"), cell("15.0"), cell("25.0")],
        ]
    }

    #[test]
    fn extracts_sample_metadata() {
        let dataset = grid_to_dataset(&sample_grid()).unwrap();
        assert_eq!(dataset.samples.label, vec!["S1", "S2"]);
        assert_eq!(dataset.samples.columns["batch"], vec!["A", "B"]);
        assert_eq!(dataset.samples.columns["sampleType"], vec!["qc", "sample"]);
        assert_eq!(dataset.samples.columns["time"], vec!["1", "2"]);
    }

    #[test]
    fn extracts_compound_metadata() {
        let dataset = grid_to_dataset(&sample_grid()).unwrap();
        assert_eq!(dataset.compounds.label, vec!["Compound1", "Compound2"]);
        assert_eq!(dataset.compounds.columns["No"], vec!["1", "2"]);
    }

    #[test]
    fn extracts_values_matrix() {
        let dataset = grid_to_dataset(&sample_grid()).unwrap();
        assert_eq!(dataset.values.shape(), &[2, 2]);
        assert_eq!(dataset.values[[0, 0]], 10.5);
        assert_eq!(dataset.values[[0, 1]], 20.5);
        assert_eq!(dataset.values[[1, 0]], 15.0);
        assert_eq!(dataset.values[[1, 1]], 25.0);
    }

    #[test]
    fn errors_when_label_field_is_missing() {
        let mut grid = sample_grid();
        grid[3][1] = None; // corner cell no longer says "label"
        let err = grid_to_dataset(&grid).unwrap_err();
        assert!(err.to_string().contains("label"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core layout::tests`
Expected: FAIL — `grid_to_dataset` not found.

- [ ] **Step 3: Write `dataset.rs`**

```rust
use ndarray::Array2;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct RawSampleTable {
    pub label: Vec<String>,
    pub columns: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawCompoundTable {
    pub label: Vec<String>,
    pub columns: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Dataset {
    pub samples: RawSampleTable,
    pub compounds: RawCompoundTable,
    pub values: Array2<f64>,
}
```

Add `pub mod dataset;` to `lib.rs`.

- [ ] **Step 4: Implement `grid_to_dataset` in `layout.rs`**

```rust
use crate::dataset::{Dataset, RawCompoundTable, RawSampleTable};
use crate::error::SerrfError;
use ndarray::Array2;
use std::collections::HashMap;

pub(crate) fn grid_to_dataset(grid: &[Vec<Option<String>>]) -> Result<Dataset, SerrfError> {
    let nrows = grid.len();
    let ncols = grid.first().map(|r| r.len()).unwrap_or(0);
    if nrows == 0 || ncols == 0 {
        return Err(SerrfError::Parse("input file is empty".into()));
    }

    let sample_col_start = grid[0]
        .iter()
        .position(|c| c.is_some())
        .ok_or_else(|| SerrfError::Parse("the first row of the file is entirely empty".into()))?;
    let compound_row_start = (0..nrows)
        .find(|&r| grid[r][0].is_some())
        .ok_or_else(|| SerrfError::Parse("the first column of the file is entirely empty".into()))?;

    // --- sample metadata: vertical field names live in the corner column ---
    let vertical_field_names: Vec<String> = (0..=compound_row_start)
        .map(|r| grid[r][sample_col_start].clone().unwrap_or_default())
        .collect();
    let field_names_p = rotate_last_to_front(&vertical_field_names);
    if field_names_p.first().map(String::as_str) != Some("label") {
        return Err(SerrfError::Parse(
            "cannot find 'label' in your data. Please check the data format requirement.".into(),
        ));
    }

    let mut sample_label = Vec::new();
    let mut sample_columns: HashMap<String, Vec<String>> = HashMap::new();
    for name in field_names_p.iter().skip(1) {
        sample_columns.entry(name.clone()).or_default();
    }
    for col in (sample_col_start + 1)..ncols {
        let raw: Vec<String> = (0..=compound_row_start)
            .map(|r| grid[r][col].clone().unwrap_or_default())
            .collect();
        let ordered = rotate_last_to_front(&raw);
        sample_label.push(if ordered[0].is_empty() { "na".to_string() } else { ordered[0].clone() });
        for (name, value) in field_names_p.iter().skip(1).zip(ordered.iter().skip(1)) {
            sample_columns.get_mut(name).unwrap().push(value.clone());
        }
    }

    // --- compound metadata: horizontal field names live in the shared header row ---
    let horizontal_field_names: Vec<String> = (0..=sample_col_start)
        .map(|c| grid[compound_row_start][c].clone().unwrap_or_default())
        .collect();
    let field_names_f = rotate_last_to_front(&horizontal_field_names);

    let mut compound_label = Vec::new();
    let mut compound_columns: HashMap<String, Vec<String>> = HashMap::new();
    for name in field_names_f.iter().skip(1) {
        compound_columns.entry(name.clone()).or_default();
    }
    for row in (compound_row_start + 1)..nrows {
        let raw: Vec<String> = (0..=sample_col_start)
            .map(|c| grid[row][c].clone().unwrap_or_default())
            .collect();
        let ordered = rotate_last_to_front(&raw);
        compound_label.push(if ordered[0].is_empty() { "na".to_string() } else { ordered[0].clone() });
        for (name, value) in field_names_f.iter().skip(1).zip(ordered.iter().skip(1)) {
            compound_columns.get_mut(name).unwrap().push(value.clone());
        }
    }

    // --- values matrix ---
    let n_compounds = compound_label.len();
    let n_samples = sample_label.len();
    let mut values = Array2::<f64>::from_elem((n_compounds, n_samples), f64::NAN);
    for (i, row) in ((compound_row_start + 1)..nrows).enumerate() {
        for (j, col) in ((sample_col_start + 1)..ncols).enumerate() {
            if let Some(raw) = &grid[row][col] {
                if let Ok(v) = raw.parse::<f64>() {
                    values[[i, j]] = v;
                }
            }
        }
    }

    Ok(Dataset {
        samples: RawSampleTable { label: sample_label, columns: sample_columns },
        compounds: RawCompoundTable { label: compound_label, columns: compound_columns },
        values,
    })
}

fn rotate_last_to_front(v: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(v.len());
    out.push(v[v.len() - 1].clone());
    out.extend_from_slice(&v[..v.len() - 1]);
    out
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Update `crates/serrf-core/src/parse/mod.rs`:

```rust
mod grid;
mod layout;
pub use grid::read_csv_grid;
pub(crate) use layout::grid_to_dataset;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p serrf-core layout::tests`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add spreadsheet layout detection and table extraction"
```

---

### Task 4: `read_data` CSV path (wires Task 2 + Task 3)

**Files:**
- Create: `crates/serrf-core/src/parse/read_data.rs`
- Modify: `crates/serrf-core/src/parse/mod.rs`

**Interfaces:**
- Consumes: `read_csv_grid` (Task 2), `grid_to_dataset` (Task 3).
- Produces: `pub fn read_data(path: &std::path::Path) -> Result<Dataset, SerrfError>` — dispatches by file extension (`.csv` for now; `.xlsx` added in Task 5).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_a_full_csv_file_end_to_end() {
        let mut file = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(file, ",batch,A,B").unwrap();
        writeln!(file, ",sampleType,qc,sample").unwrap();
        writeln!(file, ",time,1,2").unwrap();
        writeln!(file, "No,label,S1,S2").unwrap();
        writeln!(file, "1,Compound1,10.5,20.5").unwrap();
        writeln!(file, "2,Compound2,15.0,25.0").unwrap();
        let dataset = read_data(file.path()).unwrap();
        assert_eq!(dataset.samples.label, vec!["S1", "S2"]);
        assert_eq!(dataset.compounds.label, vec!["Compound1", "Compound2"]);
    }

    #[test]
    fn rejects_unsupported_extensions() {
        let file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        assert!(read_data(file.path()).is_err());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core read_data`
Expected: FAIL — `read_data` not found.

- [ ] **Step 3: Implement `read_data`**

```rust
use crate::dataset::Dataset;
use crate::error::SerrfError;
use crate::parse::{grid_to_dataset, read_csv_grid};
use std::path::Path;

pub fn read_data(path: &Path) -> Result<Dataset, SerrfError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("csv") => grid_to_dataset(&read_csv_grid(path)?),
        Some("xlsx") => grid_to_dataset(&crate::parse::read_xlsx_grid(path, 0)?),
        other => Err(SerrfError::Parse(format!("unsupported file extension: {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Note: this references `read_xlsx_grid`, added in Task 5 — for this task, temporarily stub it as
`Err(SerrfError::Xlsx("not yet implemented".into()))` in a private function so the crate compiles;
Task 5 replaces the stub with a real implementation and both tests keep passing.

Update `crates/serrf-core/src/parse/mod.rs`:

```rust
mod grid;
mod layout;
mod read_data;
pub use grid::read_csv_grid;
pub use read_data::read_data;
pub(crate) use layout::grid_to_dataset;

pub(crate) fn read_xlsx_grid(_path: &std::path::Path, _sheet: usize) -> Result<Vec<Vec<Option<String>>>, crate::error::SerrfError> {
    Err(crate::error::SerrfError::Xlsx("not yet implemented".into()))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core read_data`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Wire up read_data dispatch for CSV input"
```

---

### Task 5: XLSX grid reader + golden-file parse validation

**Files:**
- Modify: `crates/serrf-core/src/parse/mod.rs` (replace the `read_xlsx_grid` stub)
- Create: `crates/serrf-core/tests/golden_parse.rs`

**Interfaces:**
- Consumes: `grid_to_dataset` (Task 3), the golden fixtures from Task 1.
- Produces: real `read_xlsx_grid`, used transparently via `read_data`.

- [ ] **Step 1: Write the failing test**

`crates/serrf-core/tests/golden_parse.rs`:

```rust
use std::path::Path;

#[test]
fn parses_the_bundled_example_dataset() {
    let dataset = serrf_core::parse::read_data(Path::new("../../golden/example-dataset.xlsx")).unwrap();
    assert_eq!(dataset.samples.label.len(), 1299);
    assert_eq!(dataset.compounds.label.len(), 268);
    assert_eq!(dataset.values.shape(), &[268, 1299]);

    // cross-check against the reference sample metadata already exported from R
    let mut reader = csv::Reader::from_path("../../golden/expected/comb_p.csv").unwrap();
    let expected_batches: Vec<String> = reader
        .records()
        .map(|r| r.unwrap().get(1).unwrap().to_string())
        .collect();
    assert_eq!(dataset.samples.columns["batch"], expected_batches);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core --test golden_parse`
Expected: FAIL — `read_xlsx_grid` returns the "not yet implemented" error.

- [ ] **Step 3: Implement `read_xlsx_grid` with `calamine`**

Replace the stub in `crates/serrf-core/src/parse/mod.rs`:

```rust
pub(crate) fn read_xlsx_grid(path: &std::path::Path, sheet: usize) -> Result<Vec<Vec<Option<String>>>, crate::error::SerrfError> {
    use calamine::{open_workbook, Reader, Xlsx, DataType};
    let mut workbook: Xlsx<_> = open_workbook(path).map_err(|e| crate::error::SerrfError::Xlsx(e.to_string()))?;
    let sheet_name = workbook
        .sheet_names()
        .get(sheet)
        .cloned()
        .ok_or_else(|| crate::error::SerrfError::Xlsx(format!("sheet index {sheet} out of range")))?;
    let range = workbook
        .worksheet_range(&sheet_name)
        .ok_or_else(|| crate::error::SerrfError::Xlsx("sheet not found".into()))?
        .map_err(|e| crate::error::SerrfError::Xlsx(e.to_string()))?;

    let grid = range
        .rows()
        .map(|row| {
            row.iter()
                .map(|cell| match cell {
                    DataType::Empty => None,
                    DataType::String(s) if s.trim().is_empty() => None,
                    DataType::String(s) => Some(s.trim().to_string()),
                    DataType::Float(f) => Some(f.to_string()),
                    DataType::Int(i) => Some(i.to_string()),
                    other => Some(other.to_string()),
                })
                .collect()
        })
        .collect();
    Ok(grid)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core --test golden_parse`
Expected: PASS. If the batch/sample-count assertions fail, inspect the actual grid shape with a
quick debug print — the corner-detection logic in Task 3 assumes a single-row/single-column header
block; if the real file has a different header depth, adjust the test fixture in Task 3 to match
reality rather than changing the algorithm blindly.

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src crates/serrf-core/tests
git commit -m "Add XLSX grid reading and validate parsing against the bundled example dataset"
```

---

### Task 6: Validation checks

**Files:**
- Create: `crates/serrf-core/src/validate.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod validate;`)

**Interfaces:**
- Consumes: `Dataset` (Task 3).
- Produces:
  - `pub struct ValidatedSamples { pub label: Vec<String>, pub batch: Vec<String>, pub sample_type: Vec<Option<String>>, pub time: Vec<f64> }`
  - `pub fn validate(dataset: &Dataset) -> Result<ValidatedSamples, SerrfError>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, RawCompoundTable, RawSampleTable};
    use ndarray::Array2;
    use std::collections::HashMap;

    fn dataset_with_samples(columns: HashMap<String, Vec<String>>, n: usize) -> Dataset {
        Dataset {
            samples: RawSampleTable { label: (0..n).map(|i| format!("s{i}")).collect(), columns },
            compounds: RawCompoundTable { label: vec!["c1".into()], columns: HashMap::new() },
            values: Array2::from_elem((1, n), 1.0),
        }
    }

    fn valid_columns() -> HashMap<String, Vec<String>> {
        let mut cols = HashMap::new();
        cols.insert("sampleType".into(), vec!["qc","qc","qc","qc","qc","qc","sample","sample"].iter().map(|s| s.to_string()).collect());
        cols.insert("time".into(), (1..=8).map(|i| i.to_string()).collect());
        cols.insert("batch".into(), vec!["A"; 8].iter().map(|s| s.to_string()).collect());
        cols
    }

    #[test]
    fn accepts_a_valid_dataset() {
        let dataset = dataset_with_samples(valid_columns(), 8);
        let validated = validate(&dataset).unwrap();
        assert_eq!(validated.batch, vec!["A"; 8]);
        assert_eq!(validated.time, (1..=8).map(|i| i as f64).collect::<Vec<_>>());
    }

    #[test]
    fn rejects_missing_sample_type() {
        let mut cols = valid_columns();
        cols.remove("sampleType");
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("sampleType"));
    }

    #[test]
    fn rejects_sample_type_without_qc_and_sample() {
        let mut cols = valid_columns();
        cols.insert("sampleType".into(), vec!["qc"; 8].iter().map(|s| s.to_string()).collect());
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("qc"));
    }

    #[test]
    fn rejects_missing_time() {
        let mut cols = valid_columns();
        cols.remove("time");
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("time"));
    }

    #[test]
    fn rejects_duplicate_time_values() {
        let mut cols = valid_columns();
        cols.insert("time".into(), vec!["1", "1", "3", "4", "5", "6", "7", "8"].iter().map(|s| s.to_string()).collect());
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("duplicated"));
    }

    #[test]
    fn rejects_missing_batch() {
        let mut cols = valid_columns();
        cols.remove("batch");
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("batch"));
    }

    #[test]
    fn rejects_batches_with_too_few_qc() {
        let mut cols = valid_columns();
        cols.insert(
            "sampleType".into(),
            vec!["qc","qc","sample","sample","sample","sample","sample","sample"].iter().map(|s| s.to_string()).collect(),
        );
        let dataset = dataset_with_samples(cols, 8);
        assert!(validate(&dataset).unwrap_err().to_string().contains("QC"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core validate::tests`
Expected: FAIL — `validate` not found.

- [ ] **Step 3: Implement `validate`**

```rust
use crate::dataset::Dataset;
use crate::error::SerrfError;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedSamples {
    pub label: Vec<String>,
    pub batch: Vec<String>,
    pub sample_type: Vec<Option<String>>,
    pub time: Vec<f64>,
}

pub fn validate(dataset: &Dataset) -> Result<ValidatedSamples, SerrfError> {
    let sample_type_raw = dataset.samples.columns.get("sampleType").ok_or_else(|| {
        SerrfError::Validation("Your data must have 'sampleType'. Please see example data for more information.".into())
    })?;
    let sample_type: Vec<Option<String>> = sample_type_raw
        .iter()
        .map(|s| if s.trim().is_empty() { None } else { Some(s.clone()) })
        .collect();
    let has_qc = sample_type.iter().any(|t| t.as_deref() == Some("qc"));
    let has_sample = sample_type.iter().any(|t| t.as_deref() == Some("sample"));
    if !has_qc || !has_sample {
        return Err(SerrfError::Validation(
            "The 'sampleType' must contain at least 'qc' and 'sample'. Please see example data for more information.".into(),
        ));
    }

    let time_raw = dataset.samples.columns.get("time").ok_or_else(|| {
        SerrfError::Validation("Your data must have 'time'. Please see example data for more information.".into())
    })?;
    let time: Vec<f64> = time_raw
        .iter()
        .map(|s| s.parse::<f64>().map_err(|_| SerrfError::Validation(format!("'time' value '{s}' is not numeric"))))
        .collect::<Result<_, _>>()?;
    let mut seen = HashSet::new();
    for t in &time {
        if !seen.insert(t.to_bits()) {
            return Err(SerrfError::Validation(
                "Your dataset has duplicated 'time' values. 'time' of each sample should be unique.".into(),
            ));
        }
    }

    let batch = dataset
        .samples
        .columns
        .get("batch")
        .ok_or_else(|| SerrfError::Validation("Your data must have 'batch'. Please see example data for more information.".into()))?
        .clone();

    let mut qc_counts: HashMap<&str, usize> = HashMap::new();
    for (b, t) in batch.iter().zip(sample_type.iter()) {
        if t.as_deref() == Some("qc") {
            *qc_counts.entry(b.as_str()).or_insert(0) += 1;
        }
    }
    if qc_counts.values().any(|&c| c < 6) {
        return Err(SerrfError::Validation(
            "Some batches have a small number of QC that is not enough for training the model. Each batch should have at least 6 QCs.".into(),
        ));
    }

    Ok(ValidatedSamples { label: dataset.samples.label.clone(), batch, sample_type, time })
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `pub mod validate;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core validate::tests`
Expected: PASS (7 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add dataset validation checks"
```

---

### Task 7: Missing-value and infinite-row preprocessing

**Files:**
- Create: `crates/serrf-core/src/preprocess.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod preprocess;`)

**Interfaces:**
- Consumes: `ndarray::Array2<f64>` values matrix (Task 3).
- Produces:
  - `pub fn impute_missing(values: &mut ndarray::Array2<f64>)` — replaces `NaN` in each row with half that row's non-missing minimum.
  - `pub fn extract_infinite_rows(values: &ndarray::Array2<f64>) -> Vec<usize>` — indices of rows that are entirely `Inf`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn imputes_missing_values_with_half_the_row_min() {
        let mut values = array![[2.0, f64::NAN, 4.0]];
        impute_missing(&mut values);
        assert_eq!(values[[0, 1]], 1.0); // half of min(2.0, 4.0)
    }

    #[test]
    fn leaves_non_missing_values_untouched() {
        let mut values = array![[2.0, 3.0, 4.0]];
        impute_missing(&mut values);
        assert_eq!(values, array![[2.0, 3.0, 4.0]]);
    }

    #[test]
    fn finds_rows_that_are_entirely_infinite() {
        let values = array![[1.0, 2.0], [f64::INFINITY, f64::INFINITY], [3.0, f64::INFINITY]];
        assert_eq!(extract_infinite_rows(&values), vec![1]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core preprocess::tests`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement `preprocess.rs`**

```rust
use ndarray::Array2;

pub fn impute_missing(values: &mut Array2<f64>) {
    for mut row in values.rows_mut() {
        let min_nonmissing = row.iter().cloned().filter(|v| !v.is_nan()).fold(f64::INFINITY, f64::min);
        if min_nonmissing.is_finite() {
            for v in row.iter_mut() {
                if v.is_nan() {
                    *v = 0.5 * min_nonmissing;
                }
            }
        }
    }
}

pub fn extract_infinite_rows(values: &Array2<f64>) -> Vec<usize> {
    (0..values.nrows()).filter(|&i| values.row(i).iter().all(|v| v.is_infinite())).collect()
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `pub mod preprocess;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core preprocess::tests`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add missing-value imputation and infinite-row detection"
```

---

### Task 8: RSD and outlier removal

**Files:**
- Create: `crates/serrf-core/src/rsd.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod rsd;`)

**Interfaces:**
- Produces:
  - `pub fn remove_outliers(values: &[f64]) -> Vec<f64>` — Tukey boxplot rule (1.5×IQR, R's default `boxplot.stats` behavior), ignores `NaN`.
  - `pub fn rsd(values: &[f64]) -> f64` — relative standard deviation (`sd/mean`) after outlier removal.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_a_clear_outlier() {
        let values = vec![10.0, 11.0, 9.0, 10.5, 9.5, 100.0];
        let cleaned = remove_outliers(&values);
        assert!(!cleaned.contains(&100.0));
        assert_eq!(cleaned.len(), 5);
    }

    #[test]
    fn computes_rsd_after_removing_the_outlier() {
        let values = vec![10.0, 11.0, 9.0, 10.5, 9.5, 100.0];
        let result = rsd(&values);
        assert!((result - 0.0791).abs() < 1e-3);
    }

    #[test]
    fn ignores_nan_values() {
        let values = vec![10.0, f64::NAN, 10.0, 10.0];
        assert_eq!(rsd(&values), 0.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core rsd::tests`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement `rsd.rs`**

```rust
pub fn remove_outliers(values: &[f64]) -> Vec<f64> {
    let mut sorted: Vec<f64> = values.iter().cloned().filter(|v| !v.is_nan()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if sorted.len() < 4 {
        return sorted;
    }
    let q1 = quantile_type7(&sorted, 0.25);
    let q3 = quantile_type7(&sorted, 0.75);
    let iqr = q3 - q1;
    let (lo, hi) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
    sorted.into_iter().filter(|&v| v >= lo && v <= hi).collect()
}

fn quantile_type7(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let h = (n as f64 - 1.0) * p;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

pub fn rsd(values: &[f64]) -> f64 {
    let cleaned = remove_outliers(values);
    if cleaned.is_empty() {
        return f64::NAN;
    }
    let mean = cleaned.iter().sum::<f64>() / cleaned.len() as f64;
    if cleaned.len() < 2 {
        return 0.0;
    }
    let variance = cleaned.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (cleaned.len() as f64 - 1.0);
    variance.sqrt() / mean
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `pub mod rsd;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core rsd::tests`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add RSD calculation and Tukey outlier removal"
```

---

### Task 9: Spearman correlation and variable selection

**Files:**
- Create: `crates/serrf-core/src/correlation.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod correlation;`)

**Interfaces:**
- Consumes: `ndarray::Array2<f64>` (rows = compounds, cols = samples).
- Produces:
  - `pub fn spearman_corr_matrix(data: &ndarray::Array2<f64>) -> ndarray::Array2<f64>`
  - `pub fn select_variables(corr_train: &ndarray::Array2<f64>, corr_target: &ndarray::Array2<f64>, compound_index: usize, num: usize) -> Vec<usize>` — ports the widening-window intersection from `serrfR` (`while(length(sel_var)<num)`).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn ranks_tied_values_with_average_rank() {
        assert_eq!(rank(&[1.0, 2.0, 2.0, 3.0]), vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn spearman_correlation_of_a_perfectly_monotonic_pair_is_one() {
        let data = array![[1.0, 2.0, 3.0, 4.0], [10.0, 20.0, 30.0, 40.0]];
        let corr = spearman_corr_matrix(&data);
        assert!((corr[[0, 1]] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn selects_variables_present_in_both_train_and_target_top_n() {
        // compound 0's top correlates in train are {1,2,3}; in target are {2,3,4}
        let mut corr_train = ndarray::Array2::<f64>::eye(5);
        corr_train[[0, 1]] = 0.9; corr_train[[1, 0]] = 0.9;
        corr_train[[0, 2]] = 0.8; corr_train[[2, 0]] = 0.8;
        corr_train[[0, 3]] = 0.7; corr_train[[3, 0]] = 0.7;
        let mut corr_target = ndarray::Array2::<f64>::eye(5);
        corr_target[[0, 2]] = 0.9; corr_target[[2, 0]] = 0.9;
        corr_target[[0, 3]] = 0.8; corr_target[[3, 0]] = 0.8;
        corr_target[[0, 4]] = 0.7; corr_target[[4, 0]] = 0.7;

        let selected = select_variables(&corr_train, &corr_target, 0, 2);
        assert_eq!(selected, vec![2, 3]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core correlation::tests`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement `correlation.rs`**

```rust
use ndarray::Array2;

pub fn spearman_corr_matrix(data: &Array2<f64>) -> Array2<f64> {
    let n = data.nrows();
    let ranks: Vec<Vec<f64>> = (0..n).map(|i| rank(&data.row(i).to_vec())).collect();
    let mut corr = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            corr[[i, j]] = pearson(&ranks[i], &ranks[j]);
        }
    }
    corr
}

fn rank(values: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap());
    let mut ranks = vec![0.0; values.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && values[idx[j + 1]] == values[idx[i]] {
            j += 1;
        }
        let avg_rank = ((i + j) as f64 / 2.0) + 1.0;
        for k in i..=j {
            ranks[idx[k]] = avg_rank;
        }
        i = j + 1;
    }
    ranks
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum();
    let var_a: f64 = a.iter().map(|x| (x - mean_a).powi(2)).sum();
    let var_b: f64 = b.iter().map(|y| (y - mean_b).powi(2)).sum();
    if var_a == 0.0 || var_b == 0.0 { 0.0 } else { cov / (var_a.sqrt() * var_b.sqrt()) }
}

pub fn select_variables(corr_train: &Array2<f64>, corr_target: &Array2<f64>, compound_index: usize, num: usize) -> Vec<usize> {
    let n = corr_train.nrows();
    let mut l = num;
    loop {
        let top_train = top_n_by_abs(&corr_train.column(compound_index).to_vec(), l);
        let top_target = top_n_by_abs(&corr_target.column(compound_index).to_vec(), l);
        let mut sel: Vec<usize> = top_train
            .into_iter()
            .filter(|i| top_target.contains(i) && *i != compound_index)
            .collect();
        sel.sort_unstable();
        if sel.len() >= num || l >= n {
            return sel;
        }
        l += 1;
    }
}

fn top_n_by_abs(values: &[f64], n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[b].abs().partial_cmp(&values[a].abs()).unwrap());
    idx.into_iter().take(n.min(values.len())).collect()
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `pub mod correlation;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core correlation::tests`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add Spearman correlation and correlated-variable selection"
```

---

### Task 10: CART regression tree

**Files:**
- Create: `crates/serrf-core/src/tree.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `mod tree;` — internal, used by `forest.rs` in Task 11)

**Interfaces:**
- Produces:
  - `pub(crate) enum Node { Leaf { value: f64 }, Split { feature: usize, threshold: f64, left: Box<Node>, right: Box<Node> } }`
  - `pub(crate) struct TreeConfig { pub mtry: usize, pub min_node_size: usize }`
  - `pub(crate) fn build_tree(x: &[Vec<f64>], y: &[f64], indices: &[usize], config: &TreeConfig, rng: &mut impl rand::Rng) -> Node`
  - `pub(crate) fn predict(node: &Node, row: &[f64]) -> f64`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn splits_cleanly_on_a_single_informative_feature() {
        let x = vec![vec![0.0], vec![0.0], vec![1.0], vec![1.0]];
        let y = vec![1.0, 1.0, 5.0, 5.0];
        let config = TreeConfig { mtry: 1, min_node_size: 1 };
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let tree = build_tree(&x, &y, &[0, 1, 2, 3], &config, &mut rng);
        assert_eq!(predict(&tree, &[0.0]), 1.0);
        assert_eq!(predict(&tree, &[1.0]), 5.0);
    }

    #[test]
    fn returns_a_leaf_when_all_targets_are_equal() {
        let x = vec![vec![0.0], vec![1.0]];
        let y = vec![3.0, 3.0];
        let config = TreeConfig { mtry: 1, min_node_size: 1 };
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let tree = build_tree(&x, &y, &[0, 1], &config, &mut rng);
        assert_eq!(predict(&tree, &[0.0]), 3.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core tree::tests`
Expected: FAIL — `build_tree`/`predict` not found.

- [ ] **Step 3: Implement `tree.rs`**

```rust
use rand::seq::SliceRandom;
use rand::Rng;

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Leaf { value: f64 },
    Split { feature: usize, threshold: f64, left: Box<Node>, right: Box<Node> },
}

pub(crate) struct TreeConfig {
    pub mtry: usize,
    pub min_node_size: usize,
}

pub(crate) fn build_tree(x: &[Vec<f64>], y: &[f64], indices: &[usize], config: &TreeConfig, rng: &mut impl Rng) -> Node {
    if indices.len() <= config.min_node_size || is_constant(y, indices) {
        return Node::Leaf { value: mean(y, indices) };
    }
    let n_features = x[0].len();
    let mut feature_pool: Vec<usize> = (0..n_features).collect();
    feature_pool.shuffle(rng);
    let candidate_features = &feature_pool[..config.mtry.min(n_features)];

    let mut best: Option<(usize, f64, f64)> = None;
    for &feature in candidate_features {
        let mut vals: Vec<f64> = indices.iter().map(|&i| x[i][feature]).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        vals.dedup();
        for w in vals.windows(2) {
            let threshold = (w[0] + w[1]) / 2.0;
            let (left, right): (Vec<usize>, Vec<usize>) = indices.iter().copied().partition(|&i| x[i][feature] <= threshold);
            if left.is_empty() || right.is_empty() {
                continue;
            }
            let reduction = variance_reduction(y, indices, &left, &right);
            if best.map_or(true, |(_, _, best_r)| reduction > best_r) {
                best = Some((feature, threshold, reduction));
            }
        }
    }

    match best {
        None => Node::Leaf { value: mean(y, indices) },
        Some((feature, threshold, _)) => {
            let (left, right): (Vec<usize>, Vec<usize>) = indices.iter().copied().partition(|&i| x[i][feature] <= threshold);
            Node::Split {
                feature,
                threshold,
                left: Box::new(build_tree(x, y, &left, config, rng)),
                right: Box::new(build_tree(x, y, &right, config, rng)),
            }
        }
    }
}

pub(crate) fn predict(node: &Node, row: &[f64]) -> f64 {
    match node {
        Node::Leaf { value } => *value,
        Node::Split { feature, threshold, left, right } => {
            if row[*feature] <= *threshold { predict(left, row) } else { predict(right, row) }
        }
    }
}

fn mean(y: &[f64], indices: &[usize]) -> f64 {
    indices.iter().map(|&i| y[i]).sum::<f64>() / indices.len() as f64
}

fn is_constant(y: &[f64], indices: &[usize]) -> bool {
    indices.iter().all(|&i| y[i] == y[indices[0]])
}

fn variance_reduction(y: &[f64], all: &[usize], left: &[usize], right: &[usize]) -> f64 {
    let sse = |idx: &[usize]| -> f64 {
        let m = mean(y, idx);
        idx.iter().map(|&i| (y[i] - m).powi(2)).sum::<f64>()
    };
    sse(all) - sse(left) - sse(right)
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `mod tree;` to `lib.rs` (no `pub` — internal to the crate, consumed by `forest.rs`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core tree::tests`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add CART regression tree"
```

---

### Task 11: Random forest ensemble

**Files:**
- Create: `crates/serrf-core/src/forest.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod forest;`)

**Interfaces:**
- Consumes: `tree::{build_tree, predict, Node, TreeConfig}` (Task 10, same crate).
- Produces:
  - `pub struct ForestConfig { pub num_trees: usize, pub mtry: usize, pub min_node_size: usize, pub seed: u64 }`
  - `pub struct RandomForest` with `pub fn train(x: &[Vec<f64>], y: &[f64], config: &ForestConfig) -> Self` and `pub fn predict(&self, row: &[f64]) -> f64`
  - `pub fn default_mtry(n_features: usize) -> usize` — mirrors `ranger`'s regression default `max(floor(p/3), 1)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mtry_matches_rangers_regression_formula() {
        assert_eq!(default_mtry(9), 3);
        assert_eq!(default_mtry(2), 1);
        assert_eq!(default_mtry(1), 1);
    }

    #[test]
    fn predicts_close_to_a_simple_linear_relationship() {
        let x: Vec<Vec<f64>> = (0..40).map(|i| vec![i as f64]).collect();
        let y: Vec<f64> = (0..40).map(|i| 2.0 * i as f64).collect();
        let config = ForestConfig { num_trees: 50, mtry: 1, min_node_size: 2, seed: 42 };
        let forest = RandomForest::train(&x, &y, &config);
        let prediction = forest.predict(&[20.0]);
        assert!((prediction - 40.0).abs() < 5.0, "expected close to 40.0, got {prediction}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core forest::tests`
Expected: FAIL — `RandomForest`/`default_mtry` not found.

- [ ] **Step 3: Implement `forest.rs`**

```rust
use crate::tree::{build_tree, predict, Node, TreeConfig};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub struct ForestConfig {
    pub num_trees: usize,
    pub mtry: usize,
    pub min_node_size: usize,
    pub seed: u64,
}

pub struct RandomForest {
    trees: Vec<Node>,
}

impl RandomForest {
    pub fn train(x: &[Vec<f64>], y: &[f64], config: &ForestConfig) -> Self {
        let n = x.len();
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let tree_config = TreeConfig { mtry: config.mtry, min_node_size: config.min_node_size };
        let trees = (0..config.num_trees)
            .map(|_| {
                let bootstrap: Vec<usize> = (0..n).map(|_| rng.gen_range(0..n)).collect();
                build_tree(x, y, &bootstrap, &tree_config, &mut rng)
            })
            .collect();
        RandomForest { trees }
    }

    pub fn predict(&self, row: &[f64]) -> f64 {
        self.trees.iter().map(|t| predict(t, row)).sum::<f64>() / self.trees.len() as f64
    }
}

pub fn default_mtry(n_features: usize) -> usize {
    ((n_features as f64) / 3.0).floor().max(1.0) as usize
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `pub mod forest;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core forest::tests`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add random forest ensemble mirroring ranger's regression defaults"
```

---

### Task 12: SERRF per-compound per-batch normalization core

This ports `serrfR` (`app.R` lines 402-699). **Read those lines side-by-side while implementing**
— the steps below follow the same structure (zero/NA jitter fix → per-batch correlation precompute
→ per-compound loop: variable selection, RF train/predict, rescale, outlier patch → final QC/target
median rescale) but variable/function names differ since this is idiomatic Rust, not a line-by-line
transliteration.

**Files:**
- Create: `crates/serrf-core/src/serrf.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod serrf;`)

**Interfaces:**
- Consumes: `correlation::{spearman_corr_matrix, select_variables}` (Task 9), `forest::{RandomForest, ForestConfig, default_mtry}` (Task 11), `rsd::remove_outliers` (Task 8).
- Produces:
  - `pub struct GroupInput<'a> { pub train: ndarray::ArrayView2<'a, f64>, pub target: ndarray::ArrayView2<'a, f64>, pub train_batch: &'a [String], pub target_batch: &'a [String], pub num_vars: usize }` (train = QC compounds×samples, target = non-QC compounds×samples, batches aligned to columns)
  - `pub struct GroupOutput { pub normed_train: ndarray::Array2<f64>, pub normed_target: ndarray::Array2<f64> }`
  - `pub fn serrf_normalize_group(input: &GroupInput, seed: u64, progress: impl FnMut(usize, usize)) -> GroupOutput`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    /// Builds a synthetic dataset with an engineered per-batch additive drift on QC samples,
    /// which SERRF should correct so post-normalization QC RSD drops relative to raw QC RSD.
    fn synthetic_group() -> (Array2<f64>, Array2<f64>, Vec<String>, Vec<String>) {
        let n_compounds = 6;
        let n_qc = 12;
        let n_sample = 8;
        let mut train = Array2::<f64>::zeros((n_compounds, n_qc));
        let mut target = Array2::<f64>::zeros((n_compounds, n_sample));
        let mut train_batch = Vec::new();
        let mut target_batch = Vec::new();
        for j in 0..n_qc {
            let batch = if j < n_qc / 2 { "A" } else { "B" };
            train_batch.push(batch.to_string());
            let drift = if batch == "A" { 0.0 } else { 5.0 };
            for i in 0..n_compounds {
                train[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        for j in 0..n_sample {
            let batch = if j < n_sample / 2 { "A" } else { "B" };
            target_batch.push(batch.to_string());
            let drift = if batch == "A" { 0.0 } else { 5.0 };
            for i in 0..n_compounds {
                target[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        (train, target, train_batch, target_batch)
    }

    #[test]
    fn reduces_qc_rsd_relative_to_raw_when_batch_drift_is_present() {
        let (train, target, train_batch, target_batch) = synthetic_group();
        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars: 3,
        };
        let output = serrf_normalize_group(&input, 1, |_, _| {});

        let raw_rsd = crate::rsd::rsd(&train.row(0).to_vec());
        let normed_rsd = crate::rsd::rsd(&output.normed_train.row(0).to_vec());
        assert!(normed_rsd <= raw_rsd, "expected SERRF to not worsen QC RSD: raw={raw_rsd}, normed={normed_rsd}");
        assert_eq!(output.normed_train.shape(), train.shape());
        assert_eq!(output.normed_target.shape(), target.shape());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core serrf::tests`
Expected: FAIL — `serrf_normalize_group` not found.

- [ ] **Step 3: Implement `serrf.rs`**

```rust
use crate::correlation::{select_variables, spearman_corr_matrix};
use crate::forest::{default_mtry, ForestConfig, RandomForest};
use ndarray::{Array2, ArrayView2, Axis};
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

pub struct GroupInput<'a> {
    pub train: ArrayView2<'a, f64>,
    pub target: ArrayView2<'a, f64>,
    pub train_batch: &'a [String],
    pub target_batch: &'a [String],
    pub num_vars: usize,
}

pub struct GroupOutput {
    pub normed_train: Array2<f64>,
    pub normed_target: Array2<f64>,
}

pub fn serrf_normalize_group(input: &GroupInput, seed: u64, mut progress: impl FnMut(usize, usize)) -> GroupOutput {
    let n_compounds = input.train.nrows();
    let n_train = input.train.ncols();
    let n_target = input.target.ncols();

    // combined matrix: QC columns first, then target columns, matching serrfR's `all = cbind(train, target)`
    let mut all = Array2::<f64>::zeros((n_compounds, n_train + n_target));
    all.slice_mut(ndarray::s![.., 0..n_train]).assign(&input.train);
    all.slice_mut(ndarray::s![.., n_train..]).assign(&input.target);
    let is_qc: Vec<bool> = (0..n_train + n_target).map(|i| i < n_train).collect();
    let batch: Vec<String> = input.train_batch.iter().chain(input.target_batch.iter()).cloned().collect();
    let batches: Vec<String> = {
        let mut seen = Vec::new();
        for b in &batch {
            if !seen.contains(b) {
                seen.push(b.clone());
            }
        }
        seen
    };

    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // fix zeros/NAs per batch with small jitter, mirroring serrfR lines 413-421
    for b in &batches {
        let cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b).collect();
        for i in 0..n_compounds {
            let nonzero_nonnan_min = cols
                .iter()
                .map(|&c| all[[i, c]])
                .filter(|v| !v.is_nan() && *v != 0.0)
                .fold(f64::INFINITY, f64::min);
            for &c in &cols {
                if all[[i, c]] == 0.0 {
                    all[[i, c]] = nonzero_nonnan_min + 1.0 + rng.gen_range(-0.1..0.1);
                } else if all[[i, c]].is_nan() {
                    all[[i, c]] = 0.5 * nonzero_nonnan_min + 1.0 + rng.gen_range(-0.1..0.1);
                }
            }
        }
    }

    // per-batch correlation matrices among QC and among target, mirroring serrfR lines 425-443
    let mut corr_train: HashMap<String, Array2<f64>> = HashMap::new();
    let mut corr_target: HashMap<String, Array2<f64>> = HashMap::new();
    for b in &batches {
        let qc_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b && is_qc[c]).collect();
        let target_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b && !is_qc[c]).collect();
        corr_train.insert(b.clone(), spearman_corr_matrix(&all.select(Axis(1), &qc_cols)));
        corr_target.insert(b.clone(), spearman_corr_matrix(&all.select(Axis(1), &target_cols)));
    }

    let mut normalized = Array2::<f64>::zeros((n_compounds, all.ncols()));

    for j in 0..n_compounds {
        progress(j + 1, n_compounds);
        let mut row_normalized = vec![0.0; all.ncols()];

        for b in &batches {
            let batch_cols: Vec<usize> = (0..all.ncols()).filter(|&c| batch[c] == *b).collect();
            let qc_cols: Vec<usize> = batch_cols.iter().copied().filter(|&c| is_qc[c]).collect();
            let target_cols: Vec<usize> = batch_cols.iter().copied().filter(|&c| !is_qc[c]).collect();

            let selected = select_variables(&corr_train[b], &corr_target[b], j, input.num_vars);
            if selected.is_empty() {
                for &c in &batch_cols {
                    row_normalized[c] = all[[j, c]];
                }
                continue;
            }

            let train_y: Vec<f64> = qc_cols.iter().map(|&c| all[[j, c]]).collect();
            let train_y_mean = train_y.iter().sum::<f64>() / train_y.len() as f64;
            let centered_train_y: Vec<f64> = train_y.iter().map(|v| v - train_y_mean).collect();

            let train_x: Vec<Vec<f64>> = qc_cols
                .iter()
                .map(|&c| selected.iter().map(|&i| all[[i, c]]).collect())
                .collect();
            let test_x: Vec<Vec<f64>> = target_cols
                .iter()
                .map(|&c| selected.iter().map(|&i| all[[i, c]]).collect())
                .collect();

            let forest_config = ForestConfig {
                num_trees: 500,
                mtry: default_mtry(selected.len()),
                min_node_size: 5,
                seed,
            };
            let forest = RandomForest::train(&train_x, &centered_train_y, &forest_config);

            let qc_mean = train_y_mean;
            for (idx, &c) in qc_cols.iter().enumerate() {
                let predicted = forest.predict(&train_x[idx]) + qc_mean;
                row_normalized[c] = all[[j, c]] / (predicted / qc_mean);
            }

            let target_values: Vec<f64> = target_cols.iter().map(|&c| all[[j, c]]).collect();
            let target_mean = target_values.iter().sum::<f64>() / target_values.len().max(1) as f64;
            let predictions: Vec<f64> = test_x.iter().map(|row| forest.predict(row)).collect();
            let prediction_mean = if predictions.is_empty() { 0.0 } else { predictions.iter().sum::<f64>() / predictions.len() as f64 };
            for (idx, &c) in target_cols.iter().enumerate() {
                let predicted = predictions[idx] + target_mean - prediction_mean;
                let ratio = predicted / target_mean;
                row_normalized[c] = if ratio.abs() < 1e-9 { all[[j, c]] } else { all[[j, c]] / ratio };
            }

            // negative-value fix: fall back to the raw value (serrfR line 588/622)
            for &c in &target_cols {
                if row_normalized[c] < 0.0 {
                    row_normalized[c] = all[[j, c]];
                }
            }
        }

        // final rescale of QC and non-QC groups to the original overall medians (serrfR lines 594-597)
        let qc_indices: Vec<usize> = (0..all.ncols()).filter(|&c| is_qc[c]).collect();
        let target_indices: Vec<usize> = (0..all.ncols()).filter(|&c| !is_qc[c]).collect();
        rescale_to_median(&mut row_normalized, &qc_indices, &all.row(j).to_vec());
        rescale_to_median(&mut row_normalized, &target_indices, &all.row(j).to_vec());

        for c in 0..all.ncols() {
            normalized[[j, c]] = row_normalized[c];
        }
    }

    GroupOutput {
        normed_train: normalized.slice(ndarray::s![.., 0..n_train]).to_owned(),
        normed_target: normalized.slice(ndarray::s![.., n_train..]).to_owned(),
    }
}

fn rescale_to_median(row: &mut [f64], indices: &[usize], original: &[f64]) {
    let mut normed_vals: Vec<f64> = indices.iter().map(|&i| row[i]).collect();
    let mut orig_vals: Vec<f64> = indices.iter().map(|&i| original[i]).collect();
    let normed_median = median(&mut normed_vals);
    let orig_median = median(&mut orig_vals);
    if normed_median.abs() > 1e-9 {
        let factor = orig_median / normed_median;
        for &i in indices {
            row[i] *= factor;
        }
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    if n % 2 == 0 { (values[n / 2 - 1] + values[n / 2]) / 2.0 } else { values[n / 2] }
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 go here)
}
```

Add `pub mod serrf;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core serrf::tests`
Expected: PASS. If it fails because `normed_rsd > raw_rsd`, the synthetic drift in the test fixture
may be too subtle for a 6-compound/12-QC toy example — increase `n_qc`/the drift magnitude in the
test rather than weakening the assertion, since demonstrating actual error correction is the point
of this test.

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Port serrfR per-compound per-batch normalization core"
```

---

### Task 13: 5-fold cross-validation for QC RSD

**Files:**
- Create: `crates/serrf-core/src/cv.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod cv;`)

**Interfaces:**
- Consumes: `serrf::{serrf_normalize_group, GroupInput}` (Task 12).
- Produces: `pub fn cross_validate_qc(qc: &ndarray::Array2<f64>, qc_batch: &[String], folds: usize, seed: u64, num_vars: usize) -> Vec<f64>` — per-compound RSD averaged across folds, mirroring the R script's stratified-by-batch 5-fold CV over QC samples (`app.R` lines 719-772).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn returns_one_rsd_per_compound_and_all_values_are_finite() {
        let n_compounds = 4;
        let n_qc = 20;
        let mut qc = Array2::<f64>::zeros((n_compounds, n_qc));
        let mut qc_batch = Vec::new();
        for j in 0..n_qc {
            let batch = if j % 2 == 0 { "A" } else { "B" };
            qc_batch.push(batch.to_string());
            for i in 0..n_compounds {
                qc[[i, j]] = 100.0 + (j as f64 % 5.0);
            }
        }
        let result = cross_validate_qc(&qc, &qc_batch, 5, 1, 3);
        assert_eq!(result.len(), n_compounds);
        assert!(result.iter().all(|v| v.is_finite() && *v >= 0.0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core cv::tests`
Expected: FAIL — `cross_validate_qc` not found.

- [ ] **Step 3: Implement `cv.rs`**

```rust
use crate::rsd::rsd;
use crate::serrf::{serrf_normalize_group, GroupInput};
use ndarray::{Array2, Axis};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

pub fn cross_validate_qc(qc: &Array2<f64>, qc_batch: &[String], folds: usize, seed: u64, num_vars: usize) -> Vec<f64> {
    let n_compounds = qc.nrows();
    let n = qc.ncols();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut fold_rsds: Vec<Vec<f64>> = Vec::new();

    for fold in 0..folds {
        let mut indices: Vec<usize> = (0..n).collect();
        indices.shuffle(&mut rng);
        let ratio = 0.8;
        let train_count = ((n as f64) * ratio).round() as usize;
        let (train_idx, test_idx) = loop {
            let train_idx: Vec<usize> = indices[..train_count].to_vec();
            let test_idx: Vec<usize> = indices[train_count..].to_vec();
            let test_batches: std::collections::HashSet<&str> = test_idx.iter().map(|&i| qc_batch[i].as_str()).collect();
            let all_batches: std::collections::HashSet<&str> = qc_batch.iter().map(|s| s.as_str()).collect();
            if test_batches == all_batches || fold == folds - 1 {
                break (train_idx, test_idx);
            }
            indices.shuffle(&mut rng);
        };

        let train = qc.select(Axis(1), &train_idx);
        let target = qc.select(Axis(1), &test_idx);
        let train_batch: Vec<String> = train_idx.iter().map(|&i| qc_batch[i].clone()).collect();
        let target_batch: Vec<String> = test_idx.iter().map(|&i| qc_batch[i].clone()).collect();

        let input = GroupInput { train: train.view(), target: target.view(), train_batch: &train_batch, target_batch: &target_batch, num_vars };
        let output = serrf_normalize_group(&input, seed + fold as u64, |_, _| {});

        let compound_rsds: Vec<f64> = (0..n_compounds).map(|i| rsd(&output.normed_target.row(i).to_vec())).collect();
        fold_rsds.push(compound_rsds);
    }

    (0..n_compounds)
        .map(|i| {
            let values: Vec<f64> = fold_rsds.iter().map(|f| f[i]).filter(|v| v.is_finite()).collect();
            if values.is_empty() { f64::NAN } else { values.iter().sum::<f64>() / values.len() as f64 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // (test from Step 1 goes here)
}
```

Add `pub mod cv;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core cv::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add 5-fold cross-validation for QC RSD estimation"
```

---

### Task 14: PCA (before/after)

**Files:**
- Create: `crates/serrf-core/src/pca.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod pca;`)

**Interfaces:**
- Produces:
  - `pub struct PcaResult { pub pc1: Vec<f64>, pub pc2: Vec<f64> }`
  - `pub fn pca_first_two(data: &ndarray::Array2<f64>) -> PcaResult` — `data` is compounds×samples; mirrors R's `prcomp(t(data), scale.=TRUE)`, returning the first two principal component scores per sample.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn separates_two_clusters_along_the_first_component() {
        // 2 features (rows), 6 samples (cols): first 3 samples cluster near (0,0), last 3 near (10,10)
        let data = array![
            [0.0, 0.1, -0.1, 10.0, 10.1, 9.9],
            [0.0, -0.1, 0.1, 10.0, 9.9, 10.1],
        ];
        let result = pca_first_two(&data);
        let cluster_a_mean = result.pc1[0..3].iter().sum::<f64>() / 3.0;
        let cluster_b_mean = result.pc1[3..6].iter().sum::<f64>() / 3.0;
        assert!((cluster_a_mean - cluster_b_mean).abs() > 1.0, "PC1 should separate the two clusters");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core pca::tests`
Expected: FAIL — `pca_first_two` not found.

- [ ] **Step 3: Implement `pca.rs`**

```rust
use nalgebra::DMatrix;
use ndarray::Array2;

pub struct PcaResult {
    pub pc1: Vec<f64>,
    pub pc2: Vec<f64>,
}

pub fn pca_first_two(data: &Array2<f64>) -> PcaResult {
    let n_features = data.nrows();
    let n_samples = data.ncols();

    // standardize each feature (row) to zero mean, unit variance — mirrors prcomp(scale.=TRUE)
    let mut standardized = vec![0.0; n_features * n_samples];
    for i in 0..n_features {
        let row: Vec<f64> = data.row(i).to_vec();
        let mean = row.iter().sum::<f64>() / n_samples as f64;
        let variance = row.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n_samples as f64 - 1.0);
        let sd = variance.sqrt().max(1e-12);
        for (j, v) in row.iter().enumerate() {
            standardized[j * n_features + i] = (v - mean) / sd;
        }
    }

    let matrix = DMatrix::from_row_slice(n_samples, n_features, &standardized);
    let svd = matrix.svd(true, false);
    let u = svd.u.expect("U matrix from SVD");
    let singular_values = svd.singular_values;

    let pc1: Vec<f64> = (0..n_samples).map(|s| u[(s, 0)] * singular_values[0]).collect();
    let pc2: Vec<f64> = (0..n_samples).map(|s| if singular_values.len() > 1 { u[(s, 1)] * singular_values[1] } else { 0.0 }).collect();

    PcaResult { pc1, pc2 }
}

#[cfg(test)]
mod tests {
    // (test from Step 1 goes here)
}
```

Add `pub mod pca;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core pca::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Add PCA computation via SVD"
```

---

### Task 15: Full pipeline orchestration (`normalize`)

**Files:**
- Create: `crates/serrf-core/src/pipeline.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod pipeline;`)

**Interfaces:**
- Consumes: `Dataset` (Task 3), `ValidatedSamples` (Task 6), `preprocess::{impute_missing, extract_infinite_rows}` (Task 7), `rsd::rsd` (Task 8), `serrf::{serrf_normalize_group, GroupInput}` (Task 12), `cv::cross_validate_qc` (Task 13).
- Produces:
  - `pub struct SerrfConfig { pub num_vars: usize, pub seed: u64, pub cv_folds: usize }` with `impl Default`
  - `pub struct Progress { pub stage: String, pub current: usize, pub total: usize }`
  - `pub struct PipelineOutput { pub raw: ndarray::Array2<f64>, pub serrf: ndarray::Array2<f64>, pub qc_rsd_raw: Vec<f64>, pub qc_rsd_serrf: Vec<f64>, pub validate_rsd_raw: std::collections::HashMap<String, Vec<f64>>, pub validate_rsd_serrf: std::collections::HashMap<String, Vec<f64>>, pub sample_order: Vec<String> }`
  - `pub fn normalize(dataset: &Dataset, samples: &ValidatedSamples, config: &SerrfConfig, progress: impl FnMut(Progress)) -> Result<PipelineOutput, SerrfError>`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, RawCompoundTable, RawSampleTable};
    use crate::validate::ValidatedSamples;
    use ndarray::Array2;
    use std::collections::HashMap;

    fn synthetic_dataset() -> (Dataset, ValidatedSamples) {
        let n_compounds = 5;
        let n_qc = 12;
        let n_sample = 8;
        let n = n_qc + n_sample;
        let mut values = Array2::<f64>::zeros((n_compounds, n));
        let mut batch = Vec::new();
        let mut sample_type = Vec::new();
        let mut time = Vec::new();
        for j in 0..n {
            let is_qc = j < n_qc;
            let b = if j % 4 < 2 { "A" } else { "B" };
            let drift = if b == "A" { 0.0 } else { 8.0 };
            batch.push(b.to_string());
            sample_type.push(Some(if is_qc { "qc" } else { "sample" }.to_string()));
            time.push(j as f64);
            for i in 0..n_compounds {
                values[[i, j]] = 100.0 + drift + (j as f64 % 3.0);
            }
        }
        let dataset = Dataset {
            samples: RawSampleTable { label: (0..n).map(|i| format!("s{i}")).collect(), columns: HashMap::new() },
            compounds: RawCompoundTable { label: (0..n_compounds).map(|i| format!("c{i}")).collect(), columns: HashMap::new() },
            values,
        };
        let samples = ValidatedSamples { label: dataset.samples.label.clone(), batch, sample_type, time };
        (dataset, samples)
    }

    #[test]
    fn produces_correctly_shaped_output_and_improves_qc_rsd() {
        let (dataset, samples) = synthetic_dataset();
        let config = SerrfConfig { num_vars: 3, seed: 1, cv_folds: 3 };
        let output = normalize(&dataset, &samples, &config, |_| {}).unwrap();

        assert_eq!(output.raw.shape(), dataset.values.shape());
        assert_eq!(output.serrf.shape(), dataset.values.shape());
        assert_eq!(output.qc_rsd_raw.len(), 5);
        assert_eq!(output.qc_rsd_serrf.len(), 5);

        let median = |v: &[f64]| {
            let mut sorted = v.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted[sorted.len() / 2]
        };
        assert!(median(&output.qc_rsd_serrf) <= median(&output.qc_rsd_raw));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core pipeline::tests`
Expected: FAIL — `normalize`/`SerrfConfig` not found.

- [ ] **Step 3: Implement `pipeline.rs`**

```rust
use crate::cv::cross_validate_qc;
use crate::dataset::Dataset;
use crate::error::SerrfError;
use crate::preprocess::impute_missing;
use crate::rsd::rsd;
use crate::serrf::{serrf_normalize_group, GroupInput};
use crate::validate::ValidatedSamples;
use ndarray::{Array2, Axis};
use std::collections::HashMap;

pub struct SerrfConfig {
    pub num_vars: usize,
    pub seed: u64,
    pub cv_folds: usize,
}

impl Default for SerrfConfig {
    fn default() -> Self {
        Self { num_vars: 10, seed: 1, cv_folds: 5 }
    }
}

pub struct Progress {
    pub stage: String,
    pub current: usize,
    pub total: usize,
}

pub struct PipelineOutput {
    pub raw: Array2<f64>,
    pub serrf: Array2<f64>,
    pub qc_rsd_raw: Vec<f64>,
    pub qc_rsd_serrf: Vec<f64>,
    pub validate_rsd_raw: HashMap<String, Vec<f64>>,
    pub validate_rsd_serrf: HashMap<String, Vec<f64>>,
    pub sample_order: Vec<String>,
}

pub fn normalize(dataset: &Dataset, samples: &ValidatedSamples, config: &SerrfConfig, mut progress: impl FnMut(Progress)) -> Result<PipelineOutput, SerrfError> {
    let mut values = dataset.values.clone();
    impute_missing(&mut values);

    let qc_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some("qc")).collect();
    let sample_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some("sample")).collect();
    let mut validate_types: Vec<String> = samples
        .sample_type
        .iter()
        .filter_map(|t| t.clone())
        .filter(|t| t != "qc" && t != "sample")
        .collect();
    validate_types.sort();
    validate_types.dedup();

    progress(Progress { stage: "raw RSD".into(), current: 0, total: 1 });
    let qc_rsd_raw: Vec<f64> = (0..values.nrows()).map(|i| rsd(&qc_cols.iter().map(|&c| values[[i, c]]).collect::<Vec<_>>())).collect();

    let qc_matrix = values.select(Axis(1), &qc_cols);
    let sample_matrix = values.select(Axis(1), &sample_cols);
    let qc_batch: Vec<String> = qc_cols.iter().map(|&c| samples.batch[c].clone()).collect();
    let sample_batch: Vec<String> = sample_cols.iter().map(|&c| samples.batch[c].clone()).collect();

    progress(Progress { stage: "SERRF normalization".into(), current: 0, total: values.nrows() });
    let group_output = serrf_normalize_group(
        &GroupInput { train: qc_matrix.view(), target: sample_matrix.view(), train_batch: &qc_batch, target_batch: &sample_batch, num_vars: config.num_vars },
        config.seed,
        |current, total| progress(Progress { stage: "SERRF normalization".into(), current, total }),
    );

    progress(Progress { stage: "cross-validation".into(), current: 0, total: 1 });
    let qc_rsd_serrf = cross_validate_qc(&qc_matrix, &qc_batch, config.cv_folds, config.seed, config.num_vars);

    let mut validate_rsd_raw = HashMap::new();
    let mut validate_rsd_serrf = HashMap::new();
    let mut serrf = Array2::<f64>::zeros(values.raw_dim());
    for (idx, &c) in qc_cols.iter().enumerate() {
        for i in 0..values.nrows() {
            serrf[[i, c]] = group_output.normed_train[[i, idx]];
        }
    }
    for (idx, &c) in sample_cols.iter().enumerate() {
        for i in 0..values.nrows() {
            serrf[[i, c]] = group_output.normed_target[[i, idx]];
        }
    }

    for validate_type in &validate_types {
        let validate_cols: Vec<usize> = (0..values.ncols()).filter(|&c| samples.sample_type[c].as_deref() == Some(validate_type.as_str())).collect();
        let validate_matrix = values.select(Axis(1), &validate_cols);
        let validate_batch: Vec<String> = validate_cols.iter().map(|&c| samples.batch[c].clone()).collect();
        let raw_rsd: Vec<f64> = (0..values.nrows()).map(|i| rsd(&validate_cols.iter().map(|&c| values[[i, c]]).collect::<Vec<_>>())).collect();
        let group = serrf_normalize_group(
            &GroupInput { train: qc_matrix.view(), target: validate_matrix.view(), train_batch: &qc_batch, target_batch: &validate_batch, num_vars: config.num_vars },
            config.seed,
            |_, _| {},
        );
        let normed_rsd: Vec<f64> = (0..values.nrows()).map(|i| rsd(&group.normed_target.row(i).to_vec())).collect();
        for (idx, &c) in validate_cols.iter().enumerate() {
            for i in 0..values.nrows() {
                serrf[[i, c]] = group.normed_target[[i, idx]];
            }
        }
        validate_rsd_raw.insert(validate_type.clone(), raw_rsd);
        validate_rsd_serrf.insert(validate_type.clone(), normed_rsd);
    }

    // columns with no sampleType are passed through unnormalized
    for c in 0..values.ncols() {
        if samples.sample_type[c].is_none() {
            for i in 0..values.nrows() {
                serrf[[i, c]] = values[[i, c]];
            }
        }
    }

    Ok(PipelineOutput { raw: values, serrf, qc_rsd_raw, qc_rsd_serrf, validate_rsd_raw, validate_rsd_serrf, sample_order: samples.label.clone() })
}

#[cfg(test)]
mod tests {
    // (test from Step 1 goes here)
}
```

Add `pub mod pipeline;` to `lib.rs`. Re-export `normalize`, `SerrfConfig`, `Progress`, `PipelineOutput` at the crate root for ergonomic use from `serrf-cli`:

```rust
pub use pipeline::{normalize, PipelineOutput, Progress, SerrfConfig};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core pipeline::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core/src
git commit -m "Wire up full SERRF pipeline orchestration"
```

---

### Task 16: Golden-file statistical-equivalence integration test

**Files:**
- Create: `crates/serrf-core/tests/golden_normalize.rs`

**Interfaces:**
- Consumes: `parse::read_data` (Task 5), `validate::validate` (Task 6), `pipeline::normalize` (Task 15), golden fixtures (Task 1).

- [ ] **Step 1: Write the failing test**

```rust
use std::path::Path;

fn median(values: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = values.iter().cloned().filter(|v| v.is_finite()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[sorted.len() / 2]
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum();
    let var_a: f64 = a.iter().map(|x| (x - mean_a).powi(2)).sum();
    let var_b: f64 = b.iter().map(|y| (y - mean_b).powi(2)).sum();
    cov / (var_a.sqrt() * var_b.sqrt())
}

#[test]
fn serrf_output_is_statistically_equivalent_to_the_r_reference() {
    let dataset = serrf_core::parse::read_data(Path::new("../../golden/example-dataset.xlsx")).unwrap();
    let samples = serrf_core::validate::validate(&dataset).unwrap();
    let output = serrf_core::normalize(&dataset, &samples, &serrf_core::SerrfConfig::default(), |_| {}).unwrap();

    let mut reader = csv::Reader::from_path("../../golden/expected/qc-rsds.csv").unwrap();
    let expected_serrf_rsd: Vec<f64> = reader
        .records()
        .map(|r| r.unwrap().get(2).unwrap().parse::<f64>().unwrap()) // QC_SERRF column
        .collect();

    let actual_median = median(&output.qc_rsd_serrf);
    let expected_median = median(&expected_serrf_rsd);
    assert!(
        (actual_median - expected_median).abs() / expected_median < 0.5,
        "SERRF QC RSD median {actual_median} should be within 50% of the R reference {expected_median}"
    );

    let mut reader = csv::Reader::from_path("../../golden/expected/normalized-serrf.csv").unwrap();
    let mut expected_flat = Vec::new();
    for record in reader.records() {
        let record = record.unwrap();
        for cell in record.iter().skip(1) {
            expected_flat.push(cell.parse::<f64>().unwrap_or(f64::NAN));
        }
    }
    let actual_flat: Vec<f64> = output.serrf.iter().cloned().collect();
    let n = actual_flat.len().min(expected_flat.len());
    let correlation = pearson(&actual_flat[..n], &expected_flat[..n]);
    assert!(correlation > 0.8, "normalized values should correlate with the R reference, got {correlation}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core --test golden_normalize`
Expected: likely FAIL initially (this is the first time the full pipeline runs against the real
1299-sample dataset) — either a panic (fix bugs surfaced by real data, e.g. unhandled edge cases in
batch/sampleType splitting) or an assertion failure (tune the algorithm in Task 12 if the
correlation/tolerance thresholds aren't met; do not loosen the thresholds below 0.5/0.8 without
first checking for an actual bug — these values already give real room for cross-implementation RF
divergence).

- [ ] **Step 3: Debug and fix until it passes**

This step is iterative — there's no single fixed implementation to paste, since the fix depends on
what breaks against the full 1299×268 real dataset (things a small synthetic test doesn't exercise:
performance at scale, batches with different sizes, `NaN` propagation edge cases). Use
`cargo test -p serrf-core --test golden_normalize -- --nocapture` and targeted `eprintln!`
debugging in `pipeline.rs`/`serrf.rs` to isolate divergences.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core --test golden_normalize`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-core
git commit -m "Add golden-file statistical-equivalence test against the R reference"
```

---

### Task 17: `serrf-cli` binary

**Files:**
- Modify: `crates/serrf-cli/src/main.rs`
- Create: `crates/serrf-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `serrf_core::{parse::read_data, validate::validate, normalize, SerrfConfig}`.
- Produces: a binary that writes `normalized-raw.csv`, `normalized-serrf.csv`, `qc-rsds.csv` into an output directory.

- [ ] **Step 1: Write the failing test**

`crates/serrf-cli/tests/cli.rs`:

```rust
use assert_cmd::Command;
use std::path::Path;

#[test]
fn normalizes_the_bundled_example_dataset() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("serrf-cli")
        .unwrap()
        .arg(Path::new("../../golden/example-dataset.xlsx"))
        .arg("--output-dir")
        .arg(temp.path())
        .assert()
        .success();

    assert!(temp.path().join("normalized-serrf.csv").exists());
    assert!(temp.path().join("qc-rsds.csv").exists());

    let content = std::fs::read_to_string(temp.path().join("normalized-serrf.csv")).unwrap();
    assert_eq!(content.lines().count(), 269); // header + 268 compounds
}
```

Add `tempfile = "3"` to `crates/serrf-cli/Cargo.toml`'s `[dev-dependencies]`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-cli --test cli`
Expected: FAIL — the current `main.rs` doesn't accept these arguments or write any files.

- [ ] **Step 3: Implement the CLI**

```rust
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    input: PathBuf,
    #[arg(short, long, default_value = "./output")]
    output_dir: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.output_dir)?;

    let dataset = serrf_core::parse::read_data(&args.input)?;
    let samples = serrf_core::validate::validate(&dataset)?;
    let output = serrf_core::normalize(&dataset, &samples, &serrf_core::SerrfConfig::default(), |p| {
        println!("[{}] {}/{}", p.stage, p.current, p.total);
    })?;

    write_matrix_csv(&args.output_dir.join("normalized-raw.csv"), &dataset.compounds.label, &output.raw)?;
    write_matrix_csv(&args.output_dir.join("normalized-serrf.csv"), &dataset.compounds.label, &output.serrf)?;
    write_rsd_csv(&args.output_dir.join("qc-rsds.csv"), &dataset.compounds.label, &output.qc_rsd_raw, &output.qc_rsd_serrf)?;

    println!("Done. Output written to {}", args.output_dir.display());
    Ok(())
}

fn write_matrix_csv(path: &std::path::Path, labels: &[String], matrix: &ndarray::Array2<f64>) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(std::iter::once("label".to_string()).chain((0..matrix.ncols()).map(|i| format!("sample{i}"))))?;
    for (i, label) in labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        row.extend(matrix.row(i).iter().map(|v| v.to_string()));
        writer.write_record(&row)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_rsd_csv(path: &std::path::Path, labels: &[String], raw: &[f64], serrf: &[f64]) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(["label", "QC_none", "QC_SERRF"])?;
    for (i, label) in labels.iter().enumerate() {
        writer.write_record([label.clone(), raw[i].to_string(), serrf[i].to_string()])?;
    }
    writer.flush()?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-cli --test cli`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/serrf-cli
git commit -m "Add serrf-cli binary writing CSV outputs"
```

---

### Task 18: PNG report generation

**Files:**
- Create: `crates/serrf-core/src/report.rs`
- Modify: `crates/serrf-core/src/lib.rs` (add `pub mod report;`)
- Modify: `crates/serrf-cli/src/main.rs` (write `report.png` alongside the CSVs)

**Interfaces:**
- Consumes: `pca::PcaResult` (Task 14), RSD vectors (from `PipelineOutput`, Task 15).
- Produces: `pub fn render_report(path: &std::path::Path, qc_rsd_raw: &[f64], qc_rsd_serrf: &[f64], pca_before: &crate::pca::PcaResult, pca_after: &crate::pca::PcaResult, sample_type: &[Option<String>]) -> Result<(), crate::error::SerrfError>` — a PNG with a QC-RSD barplot and before/after PCA scatterplots, replacing `app.R`'s `Bar Plot and PCA plot.png`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pca::PcaResult;

    #[test]
    fn writes_a_nonempty_png_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.png");
        let pca = PcaResult { pc1: vec![1.0, 2.0, 3.0, 4.0], pc2: vec![1.0, -1.0, 1.0, -1.0] };
        let sample_type = vec![Some("qc".to_string()), Some("qc".to_string()), Some("sample".to_string()), Some("sample".to_string())];
        render_report(&path, &[0.3, 0.4], &[0.05, 0.06], &pca, &pca, &sample_type).unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p serrf-core report::tests`
Expected: FAIL — `render_report` not found.

- [ ] **Step 3: Implement `report.rs`**

```rust
use crate::error::SerrfError;
use crate::pca::PcaResult;
use plotters::prelude::*;

pub fn render_report(
    path: &std::path::Path,
    qc_rsd_raw: &[f64],
    qc_rsd_serrf: &[f64],
    pca_before: &PcaResult,
    pca_after: &PcaResult,
    sample_type: &[Option<String>],
) -> Result<(), SerrfError> {
    let root = BitMapBackend::new(path, (1200, 1200)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| SerrfError::Parse(e.to_string()))?;
    let (top, bottom) = root.split_vertically(400);

    let median = |v: &[f64]| {
        let mut sorted: Vec<f64> = v.iter().cloned().filter(|x| x.is_finite()).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if sorted.is_empty() { 0.0 } else { sorted[sorted.len() / 2] }
    };
    let raw_median = median(qc_rsd_raw) * 100.0;
    let serrf_median = median(qc_rsd_serrf) * 100.0;
    let max_val = raw_median.max(serrf_median) * 1.2;

    let mut chart = ChartBuilder::on(&top)
        .caption("QC RSD (median %)", ("sans-serif", 24))
        .margin(20)
        .x_label_area_size(30)
        .y_label_area_size(40)
        .build_cartesian_2d(0..2, 0.0..max_val)
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart.configure_mesh().draw().map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart
        .draw_series(vec![
            Rectangle::new([(0, 0.0), (1, raw_median)], BLACK.filled()),
            Rectangle::new([(1, 0.0), (2, serrf_median)], RGBColor(255, 191, 0).filled()),
        ])
        .map_err(|e| SerrfError::Parse(e.to_string()))?;

    let (before, after) = bottom.split_horizontally(600);
    draw_pca(&before, "Before", pca_before, sample_type)?;
    draw_pca(&after, "After", pca_after, sample_type)?;

    root.present().map_err(|e| SerrfError::Parse(e.to_string()))?;
    Ok(())
}

fn draw_pca(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    title: &str,
    pca: &PcaResult,
    sample_type: &[Option<String>],
) -> Result<(), SerrfError> {
    let x_range = range_with_margin(&pca.pc1);
    let y_range = range_with_margin(&pca.pc2);
    let mut chart = ChartBuilder::on(area)
        .caption(title, ("sans-serif", 20))
        .margin(20)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_cartesian_2d(x_range, y_range)
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart.configure_mesh().draw().map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart
        .draw_series(pca.pc1.iter().zip(&pca.pc2).zip(sample_type).map(|((&x, &y), t)| {
            let color = match t.as_deref() {
                Some("qc") => RED,
                Some("sample") => BLACK,
                _ => BLUE,
            };
            Circle::new((x, y), 3, color.filled())
        }))
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    Ok(())
}

fn range_with_margin(values: &[f64]) -> std::ops::Range<f64> {
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let margin = (max - min).max(1.0) * 0.1;
    (min - margin)..(max + margin)
}

#[cfg(test)]
mod tests {
    // (test from Step 1 goes here)
}
```

Add `pub mod report;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p serrf-core report::tests`
Expected: PASS

- [ ] **Step 5: Wire into `serrf-cli`**

In `crates/serrf-cli/src/main.rs`, after computing `output`, compute PCA before/after and call
`render_report`:

```rust
let sds_before: Vec<f64> = (0..dataset.values.nrows()).map(|i| std_dev(&output.raw.row(i).to_vec())).collect();
let pca_before = serrf_core::pca::pca_first_two(&filter_rows_with_variance(&output.raw, &sds_before));
let sds_after: Vec<f64> = (0..dataset.values.nrows()).map(|i| std_dev(&output.serrf.row(i).to_vec())).collect();
let pca_after = serrf_core::pca::pca_first_two(&filter_rows_with_variance(&output.serrf, &sds_after));
serrf_core::report::render_report(
    &args.output_dir.join("report.png"),
    &output.qc_rsd_raw,
    &output.qc_rsd_serrf,
    &pca_before,
    &pca_after,
    &samples.sample_type,
)?;
```

Add the small `std_dev` and `filter_rows_with_variance` helpers to `main.rs` (filter out zero-variance
compound rows before PCA, mirroring `app.R`'s `sds > 0` filter at lines 1095-1099):

```rust
fn std_dev(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() as f64 - 1.0)).sqrt()
}

fn filter_rows_with_variance(matrix: &ndarray::Array2<f64>, sds: &[f64]) -> ndarray::Array2<f64> {
    let keep: Vec<usize> = (0..sds.len()).filter(|&i| sds[i] > 0.0).collect();
    matrix.select(ndarray::Axis(0), &keep)
}
```

- [ ] **Step 6: Run the full CLI test suite to confirm nothing broke**

Run: `cargo test -p serrf-cli`
Expected: PASS (the existing `cli.rs` test still passes; report.png now also gets written, though
that test doesn't assert on it yet — acceptable, since Task 18's own unit test already covers
`render_report` directly)

- [ ] **Step 7: Commit**

```bash
git add crates/serrf-core/src crates/serrf-cli/src
git commit -m "Add PNG report generation and wire it into the CLI"
```

---

### Task 19: Coverage check and plan close-out

**Files:**
- None created — this is a verification task.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace`
Expected: all tests PASS across `serrf-core` and `serrf-cli`.

- [ ] **Step 2: Check coverage**

Run: `cargo tarpaulin --workspace --out Stdout` (or `cargo llvm-cov --workspace` if tarpaulin
doesn't work in this environment — check which is already installed before adding a new one)
Expected: 80%+ line/branch coverage for both crates. If below 80%, identify the least-covered
module in the report and add targeted tests for its uncovered branches (most likely candidates:
error paths in `parse.rs`/`validate.rs`, or edge cases in `serrf.rs`'s per-batch loop) — do not
inflate coverage with tests that don't assert real behavior.

- [ ] **Step 3: Commit any additional coverage tests, then merge readiness**

```bash
git add -A
git commit -m "Add tests to close coverage gaps" # only if Step 2 required changes
```

This branch (`feat/serrf-core`) is now ready for `superpowers:requesting-code-review` before
merging into `docs/rust-nextjs-port-design` (or directly into `master`, per your call at merge
time) — do not merge without running that review first, per standing workflow.
