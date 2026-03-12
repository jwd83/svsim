use std::path::{Path, PathBuf};

use crate::design::CompiledDesign;
use crate::diag::Result;
use crate::frontend::SvParserFrontend;

#[derive(Debug, Clone, Default)]
pub struct Compiler {
    search_paths: Vec<PathBuf>,
}

impl Compiler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_search_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.search_paths.push(path.into());
        self
    }

    pub fn push_search_path(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.search_paths.push(path.into());
        self
    }

    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    pub fn compile_file(&self, path: impl AsRef<Path>) -> Result<CompiledDesign> {
        let frontend = SvParserFrontend::new(self.search_paths.clone());
        let source_file = frontend.parse_file(path.as_ref())?;
        Ok(CompiledDesign::new(
            self.search_paths.clone(),
            vec![source_file],
        ))
    }

    pub fn compile_str(
        &self,
        _virtual_path: impl Into<PathBuf>,
        _source: &str,
    ) -> Result<CompiledDesign> {
        Err(crate::diag::Error::Unsupported(
            "compile_str is not wired yet; the first implementation slice is file-based".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Compiler;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn compile_file_finds_top_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/full_adder.sv"))
            .expect("compile design");

        assert_eq!(design.top_module(), Some("full_adder"));
        assert_eq!(design.hir().module_count(), 1);
    }
}
