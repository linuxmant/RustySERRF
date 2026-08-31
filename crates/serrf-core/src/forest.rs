use crate::tree::{build_tree, predict, Node, TreeConfig};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub struct ForestConfig {
    pub num_trees: usize,
    pub mtry: usize,
    pub min_node_size: usize,
    pub seed: u64,
}

pub struct RandomForest {
    trees: Vec<Node>,
}

impl RandomForest {
    pub fn train(x: &[Vec<f64>], y: &[f64], config: &ForestConfig) -> Self {
        let n = x.len();
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let tree_config = TreeConfig {
            mtry: config.mtry,
            min_node_size: config.min_node_size,
        };
        let trees = (0..config.num_trees)
            .map(|_| {
                let bootstrap: Vec<usize> = (0..n).map(|_| rng.gen_range(0..n)).collect();
                build_tree(x, y, &bootstrap, &tree_config, &mut rng)
            })
            .collect();
        RandomForest { trees }
    }

    pub fn predict(&self, row: &[f64]) -> f64 {
        self.trees.iter().map(|t| predict(t, row)).sum::<f64>() / self.trees.len() as f64
    }
}

pub fn default_mtry(n_features: usize) -> usize {
    ((n_features as f64) / 3.0).floor().max(1.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mtry_matches_rangers_regression_formula() {
        assert_eq!(default_mtry(9), 3);
        assert_eq!(default_mtry(2), 1);
        assert_eq!(default_mtry(1), 1);
    }

    #[test]
    fn predicts_close_to_a_simple_linear_relationship() {
        let x: Vec<Vec<f64>> = (0..40).map(|i| vec![i as f64]).collect();
        let y: Vec<f64> = (0..40).map(|i| 2.0 * i as f64).collect();
        let config = ForestConfig {
            num_trees: 50,
            mtry: 1,
            min_node_size: 2,
            seed: 42,
        };
        let forest = RandomForest::train(&x, &y, &config);
        let prediction = forest.predict(&[20.0]);
        assert!((prediction - 40.0).abs() < 5.0, "expected close to 40.0, got {prediction}");
    }
}
