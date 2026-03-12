use std::path::PathBuf;

use crate::diag::Result;
use crate::hir::{HirDesign, SourceFile};
use crate::sim::SimulationSession;

#[derive(Debug, Clone)]
pub struct CompiledDesign {
    hir: HirDesign,
    search_paths: Vec<PathBuf>,
    top_module: Option<String>,
}

impl CompiledDesign {
    pub(crate) fn new(search_paths: Vec<PathBuf>, files: Vec<SourceFile>) -> Self {
        let hir = HirDesign::new(files);
        let top_module = hir
            .files()
            .last()
            .and_then(|file| file.modules.last())
            .map(|module| module.name.clone());

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
        self.top_module.as_deref()
    }

    pub fn instantiate_top(&self) -> Result<SimulationSession> {
        let top_module = self
            .top_module
            .clone()
            .ok_or_else(|| crate::diag::Error::Parse("design contains no modules".into()))?;

        Ok(SimulationSession::new(top_module))
    }
}
