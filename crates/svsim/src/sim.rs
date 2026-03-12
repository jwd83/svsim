use std::collections::BTreeMap;

use crate::diag::{Error, Result};

#[derive(Debug, Clone)]
pub struct SimulationSession {
    top_module: String,
}

impl SimulationSession {
    pub(crate) fn new(top_module: String) -> Self {
        Self { top_module }
    }

    pub fn top_module(&self) -> &str {
        &self.top_module
    }

    pub fn eval_once(&mut self, _inputs: BTreeMap<String, u64>) -> Result<BTreeMap<String, u64>> {
        Err(Error::Unsupported(
            "the simulation engine is not implemented yet".into(),
        ))
    }

    pub fn step(&mut self, _inputs: BTreeMap<String, u64>) -> Result<BTreeMap<String, u64>> {
        Err(Error::Unsupported(
            "the sequential engine is not implemented yet".into(),
        ))
    }
}
