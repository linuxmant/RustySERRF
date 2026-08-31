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

        let input = GroupInput {
            train: train.view(),
            target: target.view(),
            train_batch: &train_batch,
            target_batch: &target_batch,
            num_vars,
        };
        let output = serrf_normalize_group(&input, seed + fold as u64, |_, _| {});

        let compound_rsds: Vec<f64> = (0..n_compounds).map(|i| rsd(&output.normed_target.row(i).to_vec())).collect();
        fold_rsds.push(compound_rsds);
    }

    (0..n_compounds)
        .map(|i| {
            let values: Vec<f64> = fold_rsds.iter().map(|f| f[i]).filter(|v| v.is_finite()).collect();
            if values.is_empty() {
                f64::NAN
            } else {
                values.iter().sum::<f64>() / values.len() as f64
            }
        })
        .collect()
}

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
