pub mod correlation;
pub mod cv;
pub mod dataset;
pub mod error;
pub mod forest;
pub mod parse;
pub mod pca;
pub mod pipeline;
pub mod preprocess;
pub mod report;
pub mod rsd;
pub mod serrf;
mod tree;
pub mod validate;

pub use pipeline::{normalize, PipelineOutput, Progress, SerrfConfig};

pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_version() {
        assert!(!crate_version().is_empty());
    }
}
