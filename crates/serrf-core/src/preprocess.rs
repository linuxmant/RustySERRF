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
