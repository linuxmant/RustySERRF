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
        if sorted.is_empty() {
            0.0
        } else {
            sorted[sorted.len() / 2]
        }
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

fn draw_pca(area: &DrawingArea<BitMapBackend, plotters::coord::Shift>, title: &str, pca: &PcaResult, sample_type: &[Option<String>]) -> Result<(), SerrfError> {
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
    use super::*;
    use crate::pca::PcaResult;

    #[test]
    fn writes_a_nonempty_png_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.png");
        let pca = PcaResult {
            pc1: vec![1.0, 2.0, 3.0, 4.0],
            pc2: vec![1.0, -1.0, 1.0, -1.0],
        };
        let sample_type = vec![
            Some("qc".to_string()),
            Some("qc".to_string()),
            Some("sample".to_string()),
            Some("sample".to_string()),
        ];
        render_report(&path, &[0.3, 0.4], &[0.05, 0.06], &pca, &pca, &sample_type).unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }
}
