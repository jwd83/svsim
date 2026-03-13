use std::path::PathBuf;

use crate::diag::Result;
use crate::hir::{HirDesign, SourceFile};
use crate::sim::SimulationSession;

#[derive(Debug, Clone)]
pub struct CompiledDesign {
    hir: HirDesign,
    search_paths: Vec<PathBuf>,
    top_module: String,
}

impl CompiledDesign {
    pub(crate) fn new(
        search_paths: Vec<PathBuf>,
        files: Vec<SourceFile>,
        top_module: String,
    ) -> Self {
        let hir = HirDesign::new(files);

        Self {
            hir,
            search_paths,
            top_module,
        }
    }

    pub fn hir(&self) -> &HirDesign {
        &self.hir
    }

    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    pub fn top_module(&self) -> Option<&str> {
        Some(self.top_module.as_str())
    }

    pub fn instantiate_top(&self) -> Result<SimulationSession> {
        Ok(SimulationSession::new(self.top_module.clone()))
    }
}
