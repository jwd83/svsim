use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;

use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::frontend::SvParserFrontend;
use crate::hir::SourceFile;
use crate::test::{
    JsonTestCorpusReport, JsonTestDirectoryReport, JsonTestDirectoryRunReport,
    JsonTestSuiteRunReport, build_corpus_report, build_directory_report,
};

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
        let source_file = frontend.parse_file(&root_path)?;
        self.compile_from_source_file(root_path, source_file)
    }

    pub fn compile_str(
        &self,
        virtual_path: impl Into<PathBuf>,
        source: &str,
    ) -> Result<CompiledDesign> {
        let frontend = SvParserFrontend::new(self.search_paths.clone());
        let virtual_path = virtual_path.into();
        let source_file = frontend.parse_str(&virtual_path, source)?;
        self.compile_from_source_file(virtual_path, source_file)
    }

    pub fn run_json_test_dir(&self, path: impl AsRef<Path>) -> Result<JsonTestDirectoryReport> {
        let started_at = Instant::now();
        let root = path.as_ref();
        let suite_paths = collect_json_test_suites(root)?;
        if suite_paths.is_empty() {
            return Err(Error::Resolve(format!(
                "no SystemVerilog/JSON regression pairs found under {}",
                root.display()
            )));
        }

        let mut suites = suite_paths
            .into_par_iter()
            .map(|suite| self.run_json_test_suite(suite))
            .collect::<Vec<_>>();
        suites.sort_by(|left, right| left.source_path.cmp(&right.source_path));

        Ok(build_directory_report(suites, started_at.elapsed()))
    }

    pub fn run_json_test_dirs(&self, paths: &[PathBuf]) -> Result<JsonTestCorpusReport> {
        let started_at = Instant::now();
        if paths.is_empty() {
            return Err(Error::Resolve(
                "at least one JSON regression directory is required".into(),
            ));
        }

        let mut directories = Vec::with_capacity(paths.len());
        for path in paths {
            directories.push(JsonTestDirectoryRunReport {
                directory: path.clone(),
                report: self.run_json_test_dir(path)?,
            });
        }

        Ok(build_corpus_report(directories, started_at.elapsed()))
    }

    fn run_json_test_suite(&self, suite: JsonTestSuitePaths) -> JsonTestSuiteRunReport {
        let started_at = Instant::now();
        match self.compile_file(&suite.source_path) {
            Ok(design) => {
                let top_module = design.top_module().map(str::to_owned);
                match design.run_json_file(&suite.json_path) {
                    Ok(report) => {
                        let passed = report.all_passed();
                        JsonTestSuiteRunReport {
                            source_path: suite.source_path,
                            json_path: suite.json_path,
                            top_module,
                            duration_ms: elapsed_millis(started_at),
                            passed,
                            report: Some(report),
                            error: None,
                        }
                    }
                    Err(error) => JsonTestSuiteRunReport {
                        source_path: suite.source_path,
                        json_path: suite.json_path,
                        top_module,
                        duration_ms: elapsed_millis(started_at),
                        passed: false,
                        report: None,
                        error: Some(error.to_string()),
                    },
                }
            }
            Err(error) => JsonTestSuiteRunReport {
                source_path: suite.source_path,
                json_path: suite.json_path,
                top_module: None,
                duration_ms: elapsed_millis(started_at),
                passed: false,
                report: None,
                error: Some(error.to_string()),
            },
        }
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

    fn compile_from_source_file(
        &self,
        root_path: PathBuf,
        root_source_file: SourceFile,
    ) -> Result<CompiledDesign> {
        let frontend = SvParserFrontend::new(self.search_paths.clone());
        let mut seen_paths = HashSet::new();
        let mut provided_modules = HashMap::<String, PathBuf>::new();
        let mut files = Vec::<SourceFile>::new();
        let mut pending_paths = Vec::new();

        let top_module = root_source_file
            .modules
            .last()
            .map(|module| module.name.clone())
            .ok_or_else(|| {
                Error::Parse(format!(
                    "design contains no modules: {}",
                    root_path.display()
                ))
            })?;

        let current_dir = root_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        seen_paths.insert(root_path.clone());
        self.register_source_file(
            &root_path,
            &root_source_file,
            &current_dir,
            &mut provided_modules,
            &mut seen_paths,
            &mut pending_paths,
        )?;
        files.push(root_source_file);

        while let Some(next_path) = pending_paths.pop() {
            let source_file = frontend.parse_file(&next_path)?;
            let current_dir = next_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            self.register_source_file(
                &next_path,
                &source_file,
                &current_dir,
                &mut provided_modules,
                &mut seen_paths,
                &mut pending_paths,
            )?;
            files.push(source_file);
        }

        Ok(CompiledDesign::new(
            self.search_paths.clone(),
            files,
            top_module,
        ))
    }

    fn register_source_file(
        &self,
        source_path: &Path,
        source_file: &SourceFile,
        current_dir: &Path,
        provided_modules: &mut HashMap<String, PathBuf>,
        seen_paths: &mut HashSet<PathBuf>,
        pending_paths: &mut Vec<PathBuf>,
    ) -> Result<()> {
        for module in &source_file.modules {
            if let Some(existing_path) =
                provided_modules.insert(module.name.clone(), source_path.to_path_buf())
            {
                if existing_path != source_path {
                    return Err(Error::Resolve(format!(
                        "module '{}' is defined in both {} and {}",
                        module.name,
                        existing_path.display(),
                        source_path.display()
                    )));
                }
            }
        }

        for module in &source_file.modules {
            for instantiation in &module.instantiations {
                if provided_modules.contains_key(&instantiation.module_name) {
                    continue;
                }
                let module_path =
                    self.resolve_module_path(&instantiation.module_name, current_dir)?;
                if seen_paths.insert(module_path.clone()) {
                    pending_paths.push(module_path);
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct JsonTestSuitePaths {
    source_path: PathBuf,
    json_path: PathBuf,
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn collect_json_test_suites(root: &Path) -> Result<Vec<JsonTestSuitePaths>> {
    let mut suites = Vec::new();
    collect_json_test_suites_recursive(root, &mut suites)?;
    suites.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok(suites)
}

fn collect_json_test_suites_recursive(
    root: &Path,
    suites: &mut Vec<JsonTestSuitePaths>,
) -> Result<()> {
    if root.is_file() {
        if root.extension().and_then(|ext| ext.to_str()) == Some("sv") {
            let json_path = root.with_extension("json");
            if json_path.is_file() {
                suites.push(JsonTestSuitePaths {
                    source_path: root.to_path_buf(),
                    json_path,
                });
            }
        }
        return Ok(());
    }

    let mut entries = fs::read_dir(root)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_json_test_suites_recursive(&path, suites)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("sv") {
            continue;
        }

        let json_path = path.with_extension("json");
        if json_path.is_file() {
            suites.push(JsonTestSuitePaths {
                source_path: path,
                json_path,
            });
        }
    }

    Ok(())
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

    #[test]
    fn compile_file_loads_overture_fetch_without_memory_diagnostics() {
        let repo = repo_root();
        let design = Compiler::new()
            .add_search_path(repo.join("parts/overture"))
            .compile_file(repo.join("parts/overture/overture_cpu.sv"))
            .expect("compile overture_cpu");

        let fetch = design
            .hir()
            .module("overture_fetch")
            .expect("overture_fetch module");
        assert!(fetch.unsupported.is_empty());
        assert_eq!(fetch.memories.len(), 1);
    }

    #[test]
    fn compile_str_finds_top_module() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                "module top(input logic a, output logic y); assign y = ~a; endmodule\n",
            )
            .expect("compile virtual design");

        assert_eq!(design.top_module(), Some("top"));
        assert_eq!(
            design
                .hir()
                .module_names()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["top"]),
        );
    }

    #[test]
    fn compile_str_uses_virtual_path_directory_for_dependencies() {
        let temp_dir = unique_temp_dir("compile-str-neighbor");
        fs::write(
            temp_dir.join("child.sv"),
            "module child(output logic outY); assign outY = 1'b1; endmodule\n",
        )
        .expect("write child");

        let design = Compiler::new()
            .compile_str(
                temp_dir.join("top.sv"),
                "module top(output logic outY); child u_child (.outY(outY)); endmodule\n",
            )
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
    fn compile_str_uses_search_paths_for_dependencies() {
        let temp_dir = unique_temp_dir("compile-str-search-path");
        let lib_dir = temp_dir.join("lib");
        fs::create_dir_all(&lib_dir).expect("create lib dir");
        fs::write(
            lib_dir.join("child.sv"),
            "module child(output logic outY); assign outY = 1'b1; endmodule\n",
        )
        .expect("write child");

        let design = Compiler::new()
            .add_search_path(&lib_dir)
            .compile_str(
                temp_dir.join("top.sv"),
                "module top(output logic outY); child u_child (.outY(outY)); endmodule\n",
            )
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
    fn run_json_test_dir_reports_passes_and_failures() {
        let temp_dir = unique_temp_dir("json-test-dir");
        let nested_dir = temp_dir.join("nested");
        fs::create_dir_all(&nested_dir).expect("create nested dir");

        fs::write(
            temp_dir.join("pass.sv"),
            "module pass(output logic one); assign one = 1'b1; endmodule\n",
        )
        .expect("write pass.sv");
        fs::write(temp_dir.join("pass.json"), "[{\"expect\":{\"one\":1}}]")
            .expect("write pass.json");

        fs::write(
            nested_dir.join("fail.sv"),
            "module fail(output logic zero); assign zero = 1'b0; endmodule\n",
        )
        .expect("write fail.sv");
        fs::write(nested_dir.join("fail.json"), "[{\"expect\":{\"zero\":1}}]")
            .expect("write fail.json");

        fs::write(
            temp_dir.join("ignored.sv"),
            "module ignored(output logic one); assign one = 1'b1; endmodule\n",
        )
        .expect("write ignored.sv");

        let report = Compiler::new()
            .run_json_test_dir(&temp_dir)
            .expect("run batch regression");

        assert_eq!(report.total, 2);
        assert_eq!(report.passed, 1);
        assert!(!report.all_passed());
        assert_eq!(
            report
                .suites
                .iter()
                .map(|suite| {
                    (
                        suite
                            .source_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .expect("file name"),
                        suite.passed,
                    )
                })
                .collect::<Vec<_>>(),
            vec![("fail.sv", false), ("pass.sv", true)],
        );
        assert!(report.suites[0].report.is_some());
        assert!(report.suites[0].error.is_none());
    }

    #[test]
    fn run_json_test_dir_keeps_compile_errors_in_report() {
        let temp_dir = unique_temp_dir("json-test-dir-compile-error");

        fs::write(
            temp_dir.join("top.sv"),
            "module top(output logic outY); missing_dep u_missing (.outY(outY)); endmodule\n",
        )
        .expect("write top.sv");
        fs::write(temp_dir.join("top.json"), "[{\"expect\":{\"outY\":1}}]")
            .expect("write top.json");

        let report = Compiler::new()
            .run_json_test_dir(&temp_dir)
            .expect("run batch regression");

        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 0);
        assert!(!report.all_passed());
        assert!(report.suites[0].report.is_none());
        assert!(
            report.suites[0]
                .error
                .as_deref()
                .is_some_and(|message| message.contains("missing_dep"))
        );
    }

    #[test]
    fn run_json_test_dirs_aggregates_directory_reports() {
        let temp_dir = unique_temp_dir("json-test-dirs");
        let passing_dir = temp_dir.join("passing");
        let failing_dir = temp_dir.join("failing");
        fs::create_dir_all(&passing_dir).expect("create passing dir");
        fs::create_dir_all(&failing_dir).expect("create failing dir");

        fs::write(
            passing_dir.join("pass.sv"),
            "module pass(output logic one); assign one = 1'b1; endmodule\n",
        )
        .expect("write passing suite");
        fs::write(passing_dir.join("pass.json"), "[{\"expect\":{\"one\":1}}]")
            .expect("write passing json");

        fs::write(
            failing_dir.join("fail.sv"),
            "module fail(output logic zero); assign zero = 1'b0; endmodule\n",
        )
        .expect("write failing suite");
        fs::write(failing_dir.join("fail.json"), "[{\"expect\":{\"zero\":1}}]")
            .expect("write failing json");

        let report = Compiler::new()
            .run_json_test_dirs(&[passing_dir.clone(), failing_dir.clone()])
            .expect("run corpus regression");

        assert_eq!(report.passed, 1);
        assert_eq!(report.total, 2);
        assert!(!report.all_passed());
        assert_eq!(
            report
                .directories
                .iter()
                .map(|directory| {
                    (
                        directory.directory.clone(),
                        directory.report.passed,
                        directory.report.total,
                    )
                })
                .collect::<Vec<_>>(),
            vec![(passing_dir, 1, 1), (failing_dir, 0, 1)],
        );
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
