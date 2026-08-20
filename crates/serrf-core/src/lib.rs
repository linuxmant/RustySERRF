pub mod correlation;
pub mod dataset;
pub mod error;
pub mod parse;
pub mod preprocess;
pub mod rsd;
pub mod validate;
mod tree;

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
