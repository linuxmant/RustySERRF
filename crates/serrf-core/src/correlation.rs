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
    idx.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
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
    if var_a == 0.0 || var_b == 0.0 {
        0.0
    } else {
        cov / (var_a.sqrt() * var_b.sqrt())
    }
}

pub fn select_variables(corr_train: &Array2<f64>, corr_target: &Array2<f64>, compound_index: usize, num: usize) -> Vec<usize> {
    let n = corr_train.nrows();
    let mut l = num;
    loop {
        let top_train = top_n_by_abs(&corr_train.column(compound_index).to_vec(), l);
        let top_target = top_n_by_abs(&corr_target.column(compound_index).to_vec(), l);
        let mut sel: Vec<usize> = top_train.into_iter().filter(|i| top_target.contains(i) && *i != compound_index).collect();
        sel.sort_unstable();
        if sel.len() >= num || l >= n {
            return sel;
        }
        l += 1;
    }
}

fn top_n_by_abs(values: &[f64], n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[b].abs().total_cmp(&values[a].abs()));
    idx.into_iter().take(n.min(values.len())).collect()
}

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
        corr_train[[0, 1]] = 0.9;
        corr_train[[1, 0]] = 0.9;
        corr_train[[0, 2]] = 0.8;
        corr_train[[2, 0]] = 0.8;
        corr_train[[0, 3]] = 0.7;
        corr_train[[3, 0]] = 0.7;
        let mut corr_target = ndarray::Array2::<f64>::eye(5);
        corr_target[[0, 2]] = 0.9;
        corr_target[[2, 0]] = 0.9;
        corr_target[[0, 3]] = 0.8;
        corr_target[[3, 0]] = 0.8;
        corr_target[[0, 4]] = 0.7;
        corr_target[[4, 0]] = 0.7;

        let selected = select_variables(&corr_train, &corr_target, 0, 2);
        assert_eq!(selected, vec![2, 3]);
    }
}
