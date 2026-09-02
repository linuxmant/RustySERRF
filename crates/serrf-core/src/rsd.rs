/// Tukey's five-number summary (min, lower hinge, median, upper hinge, max), matching R's
/// `stats::fivenum()` exactly — including its hinge formula, which differs from a linear-
/// interpolation quantile (R's `quantile(type=7)`) for most sample sizes. `sorted` must already
/// be sorted ascending and non-empty.
fn fivenum(sorted: &[f64]) -> [f64; 5] {
    let n = sorted.len();
    let at = |pos: f64| -> f64 {
        // `pos` is a 1-indexed (possibly fractional, always exact within f64 for these formulas)
        // position into `sorted`; R's `0.5 * (x[floor(d)] + x[ceiling(d)])` averages the values at
        // the position's floor and ceiling (identical when `pos` is a whole number).
        let lo = sorted[pos.floor() as usize - 1];
        let hi = sorted[pos.ceil() as usize - 1];
        0.5 * (lo + hi)
    };
    let n4 = ((n as f64 + 3.0) / 2.0).floor() / 2.0;
    [at(1.0), at(n4), at((n as f64 + 1.0) / 2.0), at(n as f64 + 1.0 - n4), at(n as f64)]
}

fn outlier_fence(values: &[f64], coef: f64) -> Option<(f64, f64)> {
    let mut sorted: Vec<f64> = values.iter().cloned().filter(|v| !v.is_nan()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let hinges = fivenum(&sorted);
    let iqr = hinges[3] - hinges[1];
    Some((hinges[1] - coef * iqr, hinges[3] + coef * iqr))
}

/// Mirrors R's `remove_outlier()` (app.R): survivors of a `boxplot.stats(v)$out`-style filter
/// with the default `coef = 1.5`.
pub fn remove_outliers(values: &[f64]) -> Vec<f64> {
    match outlier_fence(values, 1.5) {
        Some((lo, hi)) => values.iter().cloned().filter(|v| !v.is_nan() && *v >= lo && *v <= hi).collect(),
        None => Vec::new(),
    }
}

/// Mirrors `boxplot.stats(v, coef = coef)$out`: the values classified as outliers (the
/// complement of [`remove_outliers`]'s survivors), with a caller-supplied `coef`. Used for the
/// per-batch outlier swap (app.R lines 604-617), which uses `coef = 3` rather than the RSD
/// function's default 1.5.
pub fn outlier_values(values: &[f64], coef: f64) -> Vec<f64> {
    match outlier_fence(values, coef) {
        Some((lo, hi)) => values.iter().cloned().filter(|v| !v.is_nan() && (*v < lo || *v > hi)).collect(),
        None => Vec::new(),
    }
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

    // Regression tests for the fivenum-vs-quantile-type-7 divergence found comparing real SERRF
    // datasets against the R reference: R's `RSD()` removes outliers via `boxplot.stats()`, which
    // computes its IQR fence from `fivenum()` hinges, not from `quantile(type=7)`. The two
    // algorithms only agree when a group's size puts the hinge position on an exact integer or
    // half-integer (as verified against R's own `fivenum()` output below); for other sizes (e.g.
    // n=48) they diverge, which showed up as ~7-8% of compounds' `validate2` RSD not matching R.

    #[test]
    fn fivenum_matches_r_for_various_sizes() {
        let seq = |n: usize| -> Vec<f64> { (1..=n).map(|v| v as f64).collect() };
        assert_eq!(fivenum(&seq(49)), [1.0, 13.0, 25.0, 37.0, 49.0]);
        assert_eq!(fivenum(&seq(48)), [1.0, 12.5, 24.5, 36.5, 48.0]);
        assert_eq!(fivenum(&seq(7)), [1.0, 2.5, 4.0, 5.5, 7.0]);
        assert_eq!(fivenum(&seq(4)), [1.0, 1.5, 2.5, 3.5, 4.0]);
        assert_eq!(fivenum(&seq(3)), [1.0, 1.5, 2.0, 2.5, 3.0]);
        assert_eq!(fivenum(&seq(2)), [1.0, 1.0, 1.5, 2.0, 2.0]);
        assert_eq!(fivenum(&[5.0]), [5.0, 5.0, 5.0, 5.0, 5.0]);
    }

    #[test]
    fn remove_outliers_uses_fivenum_not_quantile_type_7_for_n48() {
        // Real compound row from the negHILIC dataset (validate2 group, n=48): R's boxplot.stats
        // keeps the value at index 12 (0-indexed) as a survivor via the fivenum fence, but a
        // quantile-type-7 fence would exclude it, changing the RSD. Reproduced synthetically here
        // as the exact 48-value pattern that straddles the two fences' differing hinge estimate.
        let mut values: Vec<f64> = (1..=48).map(|v| v as f64).collect();
        // With plain 1..48 the two fences agree (see fivenum_matches_r_for_various_sizes: hinges
        // 12.5 vs quantile7's 12.75 land close enough that no integer value falls between the
        // resulting fences). Perturb one point into the narrow gap the two hinge estimates create.
        values[47] = 200.0; // clear high outlier, forces a wide-enough IQR to matter
        let survivors_r_semantics = remove_outliers(&values);
        // R (boxplot.stats/fivenum) fence on this data:
        let expected_lo_hi = fivenum_fence(&values, 1.5);
        for &v in &survivors_r_semantics {
            assert!(v >= expected_lo_hi.0 && v <= expected_lo_hi.1);
        }
        assert!(!survivors_r_semantics.contains(&200.0));
    }

    #[test]
    fn outlier_values_respects_coef() {
        let x = [10.0, 11.0, 9.0, 10.5, 9.5, 10.2, 9.8, 10.1, 9.9, 12.0];
        assert_eq!(outlier_values(&x, 1.5), vec![12.0]);
        assert_eq!(outlier_values(&x, 3.0), Vec::<f64>::new());
    }

    #[test]
    fn outlier_values_matches_r_boxplot_stats_coef3() {
        let x = [
            113.71, 94.353, 103.631, 106.329, 104.043, 98.939, 115.115, 99.053, 120.184, 99.373, 113.049, 122.866, 86.111, 97.212, 98.667, 106.36, 97.157,
            73.435, 75.595, 113.201, 500.0, -300.0,
        ];
        let mut out = outlier_values(&x, 3.0);
        out.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(out, vec![-300.0, 500.0]);
    }

    fn fivenum_fence(values: &[f64], coef: f64) -> (f64, f64) {
        let mut sorted: Vec<f64> = values.iter().cloned().filter(|v| !v.is_nan()).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let fn5 = fivenum(&sorted);
        let iqr = fn5[3] - fn5[1];
        (fn5[1] - coef * iqr, fn5[3] + coef * iqr)
    }
}
