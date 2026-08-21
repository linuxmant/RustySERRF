use ndarray::Array2;

pub fn impute_missing(values: &mut Array2<f64>) {
    for mut row in values.rows_mut() {
        // Mirrors app.R:272's `e[i, is.na(e[i,])] = 0.5 * min(e[i, !is.na(e[i,])])` exactly,
        // including the degenerate all-NA-row case: R's `min(numeric(0))` is `Inf`, so
        // `0.5 * Inf == Inf`. Deliberately no `is_finite()` guard here — an all-NaN row must
        // become an all-Inf row so `extract_infinite_rows` (which only recognizes `Inf`, not
        // `NaN`) can catch it downstream and strip it before it reaches SERRF/RF training.
        let min_nonmissing = row.iter().cloned().filter(|v| !v.is_nan()).fold(f64::INFINITY, f64::min);
        for v in row.iter_mut() {
            if v.is_nan() {
                *v = 0.5 * min_nonmissing;
            }
        }
    }
}

pub fn extract_infinite_rows(values: &Array2<f64>) -> Vec<usize> {
    (0..values.nrows()).filter(|&i| values.row(i).iter().all(|v| v.is_infinite())).collect()
}

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

    #[test]
    fn an_entirely_missing_row_is_imputed_to_infinity_so_it_is_caught_as_an_infinite_row() {
        // Mirrors app.R:272's `0.5 * min(numeric(0))` == `0.5 * Inf` == `Inf`: a compound with
        // no non-missing values anywhere becomes an all-Inf row after imputation, so it flows
        // into the same strip-then-reinsert path as a genuinely all-Inf compound (C1 scenario:
        // "compound entirely missing (all NA)").
        let mut values = array![[2.0, 4.0], [f64::NAN, f64::NAN]];
        impute_missing(&mut values);
        assert_eq!(values[[0, 0]], 2.0);
        assert_eq!(values[[0, 1]], 4.0);
        assert!(values[[1, 0]].is_infinite() && values[[1, 1]].is_infinite());
        assert_eq!(extract_infinite_rows(&values), vec![1]);
    }
}
