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
        // total_cmp (not partial_cmp().unwrap()) so a non-finite feature value (e.g. a
        // correlated compound that is itself all-Inf/all-NaN, selected as a regressor by
        // serrf::serrf_normalize_group before extract_infinite_rows has a chance to strip it)
        // can never panic a tree split search. See C1's defense-in-depth fix.
        vals.sort_by(|a, b| a.total_cmp(b));
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
