use nalgebra::DMatrix;
use ndarray::Array2;

pub struct PcaResult {
    pub pc1: Vec<f64>,
    pub pc2: Vec<f64>,
}

/// Computes the first two principal component scores per sample.
///
/// `data` is compounds×samples (features are rows). Each feature is standardized
/// to zero-mean/unit-variance before SVD, mirroring R's `prcomp(t(data), scale.=TRUE)`.
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
    let pc2: Vec<f64> = (0..n_samples)
        .map(|s| {
            if singular_values.len() > 1 {
                u[(s, 1)] * singular_values[1]
            } else {
                0.0
            }
        })
        .collect();

    PcaResult { pc1, pc2 }
}

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
