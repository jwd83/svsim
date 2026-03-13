use std::path::PathBuf;

use serde::Serialize;

use crate::diag::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleInstanceSummary {
    pub module_name: String,
    pub instance_name: String,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ModuleDeclStyle {
    Ansi,
    NonAnsi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleSummary {
    pub name: String,
    pub style: ModuleDeclStyle,
    pub span: Option<SourceSpan>,
    pub instantiations: Vec<ModuleInstanceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub modules: Vec<ModuleSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct HirDesign {
    files: Vec<SourceFile>,
}

impl HirDesign {
    pub fn new(files: Vec<SourceFile>) -> Self {
        Self { files }
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn module_count(&self) -> usize {
        self.files.iter().map(|file| file.modules.len()).sum()
    }

    pub fn module_names(&self) -> Vec<&str> {
        self.files
            .iter()
            .flat_map(|file| file.modules.iter().map(|module| module.name.as_str()))
            .collect()
    }
}
