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
