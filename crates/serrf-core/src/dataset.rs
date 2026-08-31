use ndarray::Array2;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct RawSampleTable {
    pub label: Vec<String>,
    pub columns: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawCompoundTable {
    pub label: Vec<String>,
    pub columns: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Dataset {
    pub samples: RawSampleTable,
    pub compounds: RawCompoundTable,
    pub values: Array2<f64>,
}
