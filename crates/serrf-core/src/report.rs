use crate::error::SerrfError;
use crate::pca::PcaResult;
use plotters::prelude::*;
use std::collections::HashMap;

const PANEL_HEIGHT: u32 = 350;
const PCA_ROW_HEIGHT: u32 = 500;
const WIDTH: u32 = 1200;
/// @mui/x-charts's default `ScatterChart` series palette (`rainbowSurgePaletteLight`, from
/// frontend/node_modules/@mui/x-charts/colorPalettes/categorical/rainbowSurge.js) — the web UI's
/// PcaScatter.tsx never passes a `colors`/`colorScheme` prop, so this is what it actually renders
/// with. Kept in sync here so report.png's PCA colors match the live web UI for the same job.
const WEB_PALETTE: [RGBColor; 7] = [
    RGBColor(0x42, 0x54, 0xFB),
    RGBColor(0xFF, 0xB4, 0x22),
    RGBColor(0xFA, 0x4F, 0x58),
    RGBColor(0x0D, 0xBE, 0xFF),
    RGBColor(0x22, 0xBF, 0x75),
    RGBColor(0xFA, 0x83, 0xB4),
    RGBColor(0xFF, 0x75, 0x11),
];

/// Total canvas height for `num_panels` stacked RSD bar panels (QC plus one per validate type)
/// above the fixed-height before/after PCA row, mirroring the layout of R's
/// "Bar Plot and PCA plot.png" (app.R lines 1037-1120), which grows the image
/// (`height = 1000 * ifelse(with_validate, 3, 2)`) to fit however many validate groups exist
/// rather than squeezing them into a fixed-size canvas.
fn image_height(num_panels: usize) -> u32 {
    PANEL_HEIGHT * num_panels as u32 + PCA_ROW_HEIGHT
}

/// Assigns each category a color by its position in `categories`, cycling through
/// [`WEB_PALETTE`] — matching how @mui/x-charts assigns colors to `ScatterChart` series by index.
/// `categories` must be [`present_categories`]'s output for the *same* `sample_type` used to
/// build the chart, so the index (and thus the color) lines up with the web UI's.
fn color_for_category(category: &str, categories: &[String]) -> RGBColor {
    let idx = categories.iter().position(|c| c == category).unwrap_or(0);
    WEB_PALETTE[idx % WEB_PALETTE.len()]
}

/// Whether a sample-type's PCA marker should be drawn hollow (outline only) rather than filled,
/// mirroring app.R's `dots = c(1, 16, ...)`: 'sample' uses pch=1 (hollow), every other group
/// (qc, each validate type) uses pch=16 (filled). Independent of color: the web UI doesn't draw
/// hollow markers at all, but this distinction is still worth keeping for report.png's fidelity
/// to R's own plot.
fn is_hollow_marker(sample_type: Option<&str>) -> bool {
    sample_type == Some("sample")
}

/// Ordered, de-duplicated list of sample-type category names, in the order they first appear in
/// `sample_type` — matching the web UI's PcaScatter.tsx (`Array.from(new
/// Set(sampleType.map(...)))`), so report.png's legend and colors agree with the live chart for
/// the same job rather than using a fixed sample/qc/validate precedence.
fn present_categories(sample_type: &[Option<String>]) -> Vec<String> {
    let mut categories: Vec<String> = Vec::new();
    for t in sample_type.iter().flatten() {
        if !categories.contains(t) {
            categories.push(t.clone());
        }
    }
    categories
}

fn median(v: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = v.iter().cloned().filter(|x| x.is_finite()).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if sorted.is_empty() {
        0.0
    } else {
        sorted[sorted.len() / 2]
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_report(
    path: &std::path::Path,
    qc_rsd_raw: &[f64],
    qc_rsd_serrf: &[f64],
    validate_rsd_raw: &HashMap<String, Vec<f64>>,
    validate_rsd_serrf: &HashMap<String, Vec<f64>>,
    pca_before: &PcaResult,
    pca_after: &PcaResult,
    sample_type: &[Option<String>],
) -> Result<(), SerrfError> {
    let mut validate_types: Vec<String> = validate_rsd_raw.keys().cloned().collect();
    validate_types.sort();

    let mut panels: Vec<(String, &[f64], &[f64])> = vec![("QC".to_string(), qc_rsd_raw, qc_rsd_serrf)];
    for t in &validate_types {
        panels.push((format!("{t} Sample"), &validate_rsd_raw[t], &validate_rsd_serrf[t]));
    }

    let height = image_height(panels.len());
    let root = BitMapBackend::new(path, (WIDTH, height)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| SerrfError::Parse(e.to_string()))?;
    let (bars_area, pca_row) = root.split_vertically(PANEL_HEIGHT * panels.len() as u32);

    let bar_areas = bars_area.split_evenly((panels.len(), 1));
    for (area, (title, raw, serrf)) in bar_areas.iter().zip(panels.iter()) {
        draw_rsd_bars(area, title, raw, serrf)?;
    }

    let (before, after) = pca_row.split_horizontally(WIDTH / 2);
    draw_pca(&before, "Before", "raw data", pca_before, sample_type)?;
    draw_pca(&after, "After", "SERRF", pca_after, sample_type)?;

    root.present().map_err(|e| SerrfError::Parse(e.to_string()))?;
    Ok(())
}

fn draw_rsd_bars(area: &DrawingArea<BitMapBackend, plotters::coord::Shift>, title: &str, raw: &[f64], serrf: &[f64]) -> Result<(), SerrfError> {
    let raw_median = median(raw) * 100.0;
    let serrf_median = median(serrf) * 100.0;
    let max_val = (raw_median.max(serrf_median) * 1.3).max(1.0);

    let mut chart = ChartBuilder::on(area)
        .caption(format!("{title} RSD"), ("sans-serif", 24))
        .margin(20)
        .x_label_area_size(30)
        .y_label_area_size(40)
        .build_cartesian_2d(0..2, 0.0..max_val)
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart
        .configure_mesh()
        .disable_mesh()
        .x_labels(2)
        .x_label_formatter(&|x| match x {
            0 => "none".to_string(),
            1 => "SERRF".to_string(),
            _ => String::new(),
        })
        .y_desc("RSD (%)")
        .draw()
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart
        .draw_series(vec![
            Rectangle::new([(0, 0.0), (1, raw_median)], BLACK.filled()),
            Rectangle::new([(1, 0.0), (2, serrf_median)], RGBColor(255, 191, 0).filled()),
        ])
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    // Labels sit above each bar's top edge (like R's `text(..., pos = 3)`), not on top of it —
    // placed directly on the bar, a black-on-black "none" label would be invisible.
    let label_margin = max_val * 0.03;
    chart
        .draw_series(
            [(0, raw_median), (1, serrf_median)]
                .iter()
                .map(|&(x, v)| Text::new(format!("{v:.2}%"), (x, v + label_margin), ("sans-serif", 18).into_font())),
        )
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    Ok(())
}

fn draw_pca(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    title: &str,
    data_desc: &str,
    pca: &PcaResult,
    sample_type: &[Option<String>],
) -> Result<(), SerrfError> {
    let x_range = range_with_margin(&pca.pc1);
    let y_range = range_with_margin(&pca.pc2);
    let mut chart = ChartBuilder::on(area)
        .caption(title, ("sans-serif", 20))
        .margin(20)
        .x_label_area_size(40)
        .y_label_area_size(40)
        .build_cartesian_2d(x_range, y_range)
        .map_err(|e| SerrfError::Parse(e.to_string()))?;
    chart
        .configure_mesh()
        .disable_mesh()
        .x_desc(format!("PC1 ({data_desc})"))
        .y_desc("PC2")
        .draw()
        .map_err(|e| SerrfError::Parse(e.to_string()))?;

    // One labeled+legended series per category (rather than one flat, unlabeled point cloud) so
    // configure_series_labels() below can render a legend mapping each color to its group name.
    let categories = present_categories(sample_type);
    for category in &categories {
        let color = color_for_category(category, &categories);
        let hollow = is_hollow_marker(Some(category.as_str()));
        let points: Vec<(f64, f64)> = pca
            .pc1
            .iter()
            .zip(&pca.pc2)
            .zip(sample_type)
            .filter(|(_, t)| t.as_deref() == Some(category.as_str()))
            .map(|((&x, &y), _)| (x, y))
            .collect();
        chart
            .draw_series(points.iter().map(|&(x, y)| {
                if hollow {
                    Circle::new((x, y), 3, Into::<ShapeStyle>::into(color))
                } else {
                    Circle::new((x, y), 3, color.filled())
                }
            }))
            .map_err(|e| SerrfError::Parse(e.to_string()))?
            .label(category.clone())
            .legend(move |(x, y)| {
                if hollow {
                    Circle::new((x, y), 4, Into::<ShapeStyle>::into(color))
                } else {
                    Circle::new((x, y), 4, color.filled())
                }
            });
    }
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()
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
    use super::*;
    use crate::pca::PcaResult;
    use std::collections::HashMap;

    fn sample_pca() -> PcaResult {
        PcaResult {
            pc1: vec![1.0, 2.0, 3.0, 4.0],
            pc2: vec![1.0, -1.0, 1.0, -1.0],
        }
    }

    fn sample_types() -> Vec<Option<String>> {
        vec![
            Some("qc".to_string()),
            Some("qc".to_string()),
            Some("sample".to_string()),
            Some("sample".to_string()),
        ]
    }

    #[test]
    fn writes_a_nonempty_png_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.png");
        let pca = sample_pca();
        render_report(&path, &[0.3, 0.4], &[0.05, 0.06], &HashMap::new(), &HashMap::new(), &pca, &pca, &sample_types()).unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }

    #[test]
    fn writes_a_taller_report_with_more_validate_groups() {
        // R's "Bar Plot and PCA plot.png" stacks one bar panel per group (QC plus one per
        // validate type) above the PCA row; the image must grow to fit them all rather than
        // squeezing every panel into a fixed-height canvas.
        let dir = tempfile::tempdir().unwrap();
        let pca = sample_pca();

        let no_validate_path = dir.path().join("no-validate.png");
        render_report(
            &no_validate_path,
            &[0.3],
            &[0.05],
            &HashMap::new(),
            &HashMap::new(),
            &pca,
            &pca,
            &sample_types(),
        )
        .unwrap();

        let mut validate_raw = HashMap::new();
        validate_raw.insert("validate".to_string(), vec![0.3]);
        validate_raw.insert("validate2".to_string(), vec![0.3]);
        let mut validate_serrf = HashMap::new();
        validate_serrf.insert("validate".to_string(), vec![0.05]);
        validate_serrf.insert("validate2".to_string(), vec![0.05]);
        let two_validate_path = dir.path().join("two-validate.png");
        render_report(&two_validate_path, &[0.3], &[0.05], &validate_raw, &validate_serrf, &pca, &pca, &sample_types()).unwrap();

        assert_eq!(png_dimensions(&no_validate_path).1, image_height(1));
        assert_eq!(png_dimensions(&two_validate_path).1, image_height(3));
        assert!(image_height(3) > image_height(1));
    }

    fn png_dimensions(path: &std::path::Path) -> (u32, u32) {
        let bytes = std::fs::read(path).unwrap();
        // PNG IHDR: 8-byte signature, then a 4-byte length + "IHDR" + 4-byte width + 4-byte height.
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        (width, height)
    }

    #[test]
    fn image_height_grows_with_more_panels() {
        assert!(image_height(1) < image_height(2));
        assert!(image_height(2) < image_height(3));
    }

    #[test]
    fn present_categories_orders_by_first_appearance_not_by_fixed_type_precedence() {
        // Matches the web UI's PcaScatter.tsx: `Array.from(new Set(sampleType.map(...)))` groups
        // by order of first appearance in the data, not a fixed sample/qc/validate precedence —
        // report.png's legend/colors must use the same order so the two views agree.
        let sample_type = vec![Some("validate2".to_string()), Some("sample".to_string()), Some("validate2".to_string())];
        assert_eq!(present_categories(&sample_type), vec!["validate2".to_string(), "sample".to_string()]);
    }

    #[test]
    fn present_categories_includes_every_distinct_group_once() {
        let sample_type = vec![
            Some("qc".to_string()),
            Some("sample".to_string()),
            Some("validate".to_string()),
            Some("validate2".to_string()),
            Some("qc".to_string()),
        ];
        assert_eq!(
            present_categories(&sample_type),
            vec!["qc".to_string(), "sample".to_string(), "validate".to_string(), "validate2".to_string()]
        );
    }

    #[test]
    fn color_for_category_matches_muis_default_scatter_chart_palette() {
        // frontend/node_modules/@mui/x-charts/colorPalettes/categorical/rainbowSurge.js's
        // `rainbowSurgePaletteLight` — @mui/x-charts's default series palette, used unchanged by
        // PcaScatter.tsx (it passes no `colors`/`colorScheme` prop). First category -> first
        // color, matching MUI assigning colors by series index.
        let categories = vec!["qc".to_string(), "sample".to_string()];
        assert_eq!(color_for_category("qc", &categories), RGBColor(0x42, 0x54, 0xFB));
        assert_eq!(color_for_category("sample", &categories), RGBColor(0xFF, 0xB4, 0x22));
    }

    #[test]
    fn color_for_category_is_stable_and_distinct_per_category() {
        let categories = vec!["validate".to_string(), "validate2".to_string()];
        let c1 = color_for_category("validate", &categories);
        let c2 = color_for_category("validate2", &categories);
        assert_ne!(c1, c2, "different categories must get different colors");
        assert_eq!(color_for_category("validate", &categories), c1, "same category must always get the same color");
    }

    #[test]
    fn only_the_sample_group_is_drawn_hollow() {
        // Mirrors app.R's `dots = c(1, 16, ...)`: 'sample' points use pch=1 (hollow), every other
        // group (qc, each validate type) uses pch=16 (filled).
        assert!(is_hollow_marker(Some("sample")));
        assert!(!is_hollow_marker(Some("qc")));
        assert!(!is_hollow_marker(Some("validate")));
        assert!(!is_hollow_marker(Some("validate2")));
        assert!(!is_hollow_marker(None));
    }
}
