use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::frontend::SvParserFrontend;
use crate::hir::SourceFile;

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
        let root_path = fs::canonicalize(path.as_ref())?;
        let mut seen_paths = HashSet::new();
        let mut provided_modules = HashMap::<String, PathBuf>::new();
        let mut files = Vec::<SourceFile>::new();
        let mut pending_paths = vec![root_path.clone()];
        let mut top_module = None;

        while let Some(next_path) = pending_paths.pop() {
            if !seen_paths.insert(next_path.clone()) {
                continue;
            }

            let source_file = frontend.parse_file(&next_path)?;
            if top_module.is_none() {
                top_module = source_file.modules.last().map(|module| module.name.clone());
            }

            for module in &source_file.modules {
                if let Some(existing_path) =
                    provided_modules.insert(module.name.clone(), next_path.clone())
                {
                    if existing_path != next_path {
                        return Err(Error::Resolve(format!(
                            "module '{}' is defined in both {} and {}",
                            module.name,
                            existing_path.display(),
                            next_path.display()
                        )));
                    }
                }
            }

            let mut pending_modules = Vec::new();
            for module in &source_file.modules {
                for instantiation in &module.instantiations {
                    if !provided_modules.contains_key(&instantiation.module_name) {
                        pending_modules.push(instantiation.module_name.clone());
                    }
                }
            }

            let current_dir = next_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            for module_name in pending_modules {
                let module_path = self.resolve_module_path(&module_name, &current_dir)?;
                if !seen_paths.contains(&module_path) {
                    pending_paths.push(module_path);
                }
            }

            files.push(source_file);
        }

        let top_module = top_module.ok_or_else(|| {
            Error::Parse(format!(
                "design contains no modules: {}",
                root_path.display()
            ))
        })?;
        Ok(CompiledDesign::new(
            self.search_paths.clone(),
            files,
            top_module,
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

    fn resolve_module_path(&self, module_name: &str, current_dir: &Path) -> Result<PathBuf> {
        let mut candidates = Vec::new();
        candidates.push(current_dir.join(format!("{module_name}.sv")));
        for search_path in &self.search_paths {
            candidates.push(search_path.join(format!("{module_name}.sv")));
        }

        for candidate in candidates {
            if candidate.exists() {
                return Ok(fs::canonicalize(candidate)?);
            }
        }

        Err(Error::Resolve(format!(
            "module '{}' was not found next to {} or in search paths [{}]",
            module_name,
            current_dir.display(),
            self.search_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::Compiler;
    use crate::diag::Error;

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
        assert_eq!(
            design
                .hir()
                .module_names()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "and_gate",
                "full_adder",
                "half_adder",
                "nand_gate",
                "not_gate",
                "or_gate",
                "xor_gate",
            ]),
        );
    }

    #[test]
    fn compile_file_uses_search_paths_for_dependencies() {
        let temp_dir = unique_temp_dir("search-path");
        let lib_dir = temp_dir.join("lib");
        fs::create_dir_all(&lib_dir).expect("create lib dir");

        fs::write(
            temp_dir.join("top.sv"),
            "module top(output logic outY); child u_child (.outY(outY)); endmodule\n",
        )
        .expect("write top");
        fs::write(
            lib_dir.join("child.sv"),
            "module child(output logic outY); assign outY = 1'b1; endmodule\n",
        )
        .expect("write child");

        let design = Compiler::new()
            .add_search_path(&lib_dir)
            .compile_file(temp_dir.join("top.sv"))
            .expect("compile design");

        assert_eq!(
            design
                .hir()
                .module_names()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["child", "top"]),
        );
    }

    #[test]
    fn compile_file_errors_on_missing_dependency() {
        let temp_dir = unique_temp_dir("missing-dependency");
        fs::write(
            temp_dir.join("top.sv"),
            "module top(output logic outY); missing_dep u_missing (.outY(outY)); endmodule\n",
        )
        .expect("write top");

        let error = Compiler::new()
            .compile_file(temp_dir.join("top.sv"))
            .expect_err("missing dependency should fail");

        match error {
            Error::Resolve(message) => {
                assert!(
                    message.contains("missing_dep"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        path.push(format!("svsim-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}
