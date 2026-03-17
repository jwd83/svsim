use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::design::CompiledDesign;
use crate::diag::{Diagnostic, Error, Result};
use crate::frontend::SvParserFrontend;
use crate::hir::SourceFile;
use crate::test::{
    JsonTestCorpusReport, JsonTestDirectoryReport, JsonTestDirectoryRunReport,
    JsonTestSuiteRunReport, build_corpus_report, build_directory_report,
};
use crate::validate::validate_design;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompileFileReport {
    pub source_path: PathBuf,
    pub top_module: Option<String>,
    pub module_count: usize,
    pub duration_ms: u64,
    pub passed: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompileDirectoryReport {
    pub duration_ms: u64,
    pub passed: usize,
    pub total: usize,
    pub files: Vec<CompileFileReport>,
}

impl CompileDirectoryReport {
    pub fn all_passed(&self) -> bool {
        self.passed == self.total
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompileCorpusReport {
    pub duration_ms: u64,
    pub passed: usize,
    pub total: usize,
    pub directories: Vec<CompileDirectoryRunReport>,
}

impl CompileCorpusReport {
    pub fn all_passed(&self) -> bool {
        self.passed == self.total
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompileDirectoryRunReport {
    pub directory: PathBuf,
    pub report: CompileDirectoryReport,
}

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

    pub fn run_compile_dir(&self, path: impl AsRef<Path>) -> Result<CompileDirectoryReport> {
        let started_at = Instant::now();
        let root = path.as_ref();
        let source_paths = collect_systemverilog_sources(root)?;
        if source_paths.is_empty() {
            return Err(Error::Resolve(format!(
                "no SystemVerilog source files found under {}",
                root.display()
            )));
        }

        let mut files = source_paths
            .into_par_iter()
            .map(|source_path| self.run_compile_file(source_path))
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.source_path.cmp(&right.source_path));

        Ok(build_compile_directory_report(files, started_at.elapsed()))
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

    pub fn run_compile_dirs(&self, paths: &[PathBuf]) -> Result<CompileCorpusReport> {
        let started_at = Instant::now();
        if paths.is_empty() {
            return Err(Error::Resolve(
                "at least one compile-only directory is required".into(),
            ));
        }

        let mut directories = Vec::with_capacity(paths.len());
        for path in paths {
            directories.push(CompileDirectoryRunReport {
                directory: path.clone(),
                report: self.run_compile_dir(path)?,
            });
        }

        Ok(build_compile_corpus_report(
            directories,
            started_at.elapsed(),
        ))
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

    fn run_compile_file(&self, source_path: PathBuf) -> CompileFileReport {
        let started_at = Instant::now();
        match self.compile_file(&source_path) {
            Ok(design) => {
                let diagnostics = collect_unsupported_diagnostics(&design);
                let passed = diagnostics.is_empty();
                CompileFileReport {
                    source_path,
                    top_module: design.top_module().map(str::to_owned),
                    module_count: design.hir().module_count(),
                    duration_ms: elapsed_millis(started_at),
                    passed,
                    diagnostics,
                    error: None,
                }
            }
            Err(error) => CompileFileReport {
                source_path,
                top_module: None,
                module_count: 0,
                duration_ms: elapsed_millis(started_at),
                passed: false,
                diagnostics: Vec::new(),
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

        let design = CompiledDesign::new(self.search_paths.clone(), files, top_module);
        validate_design(design.hir())?;
        Ok(design)
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

#[derive(Debug, Deserialize)]
struct JsonTestSuiteSourceMetadata {
    #[serde(default)]
    source: Option<PathBuf>,
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn build_compile_directory_report(
    files: Vec<CompileFileReport>,
    duration: std::time::Duration,
) -> CompileDirectoryReport {
    let passed = files.iter().filter(|file| file.passed).count();
    let total = files.len();
    CompileDirectoryReport {
        duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        passed,
        total,
        files,
    }
}

fn build_compile_corpus_report(
    directories: Vec<CompileDirectoryRunReport>,
    duration: std::time::Duration,
) -> CompileCorpusReport {
    let passed = directories
        .iter()
        .map(|directory| directory.report.passed)
        .sum();
    let total = directories
        .iter()
        .map(|directory| directory.report.total)
        .sum();
    CompileCorpusReport {
        duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        passed,
        total,
        directories,
    }
}

fn collect_unsupported_diagnostics(design: &CompiledDesign) -> Vec<Diagnostic> {
    design
        .hir()
        .files()
        .iter()
        .flat_map(|file| file.modules.iter())
        .flat_map(|module| module.unsupported.iter().cloned())
        .collect()
}

fn collect_systemverilog_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut sources = Vec::new();
    collect_systemverilog_sources_recursive(root, &mut sources)?;
    sources.sort();
    Ok(sources)
}

fn collect_systemverilog_sources_recursive(root: &Path, sources: &mut Vec<PathBuf>) -> Result<()> {
    if root.is_file() {
        collect_systemverilog_source_file(root, sources);
        return Ok(());
    }

    let mut entries = fs::read_dir(root)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_systemverilog_sources_recursive(&path, sources)?;
            continue;
        }

        collect_systemverilog_source_file(&path, sources);
    }

    Ok(())
}

fn collect_systemverilog_source_file(path: &Path, sources: &mut Vec<PathBuf>) {
    if path.extension().and_then(|ext| ext.to_str()) == Some("sv") {
        sources.push(path.to_path_buf());
    }
}

fn collect_json_test_suites(root: &Path) -> Result<Vec<JsonTestSuitePaths>> {
    let mut suites = Vec::new();
    collect_json_test_suites_recursive(root, &mut suites)?;
    suites.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then_with(|| left.json_path.cmp(&right.json_path))
    });
    Ok(suites)
}

fn collect_json_test_suites_recursive(
    root: &Path,
    suites: &mut Vec<JsonTestSuitePaths>,
) -> Result<()> {
    if root.is_file() {
        collect_json_test_suite_file(root, suites)?;
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

        collect_json_test_suite_file(&path, suites)?;
    }

    Ok(())
}

fn collect_json_test_suite_file(path: &Path, suites: &mut Vec<JsonTestSuitePaths>) -> Result<()> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("sv") => {
            let json_path = path.with_extension("json");
            if json_path.is_file() {
                suites.push(JsonTestSuitePaths {
                    source_path: path.to_path_buf(),
                    json_path,
                });
            }
        }
        Some("json") => {
            let Some(source_path) = resolve_json_test_source_path(path)? else {
                return Ok(());
            };
            if source_path != path.with_extension("sv") {
                suites.push(JsonTestSuitePaths {
                    source_path,
                    json_path: path.to_path_buf(),
                });
            }
        }
        _ => {}
    }

    Ok(())
}

fn resolve_json_test_source_path(json_path: &Path) -> Result<Option<PathBuf>> {
    let sibling_source = json_path.with_extension("sv");
    if sibling_source.is_file() {
        return Ok(Some(sibling_source));
    }

    let text = fs::read_to_string(json_path)?;
    let Ok(metadata) = serde_json::from_str::<JsonTestSuiteSourceMetadata>(&text) else {
        return Ok(None);
    };
    let Some(source) = metadata.source else {
        return Ok(None);
    };

    let source_path = if source.is_absolute() {
        source
    } else {
        json_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(source)
    };
    if !source_path.is_file() {
        return Err(Error::Resolve(format!(
            "json test '{}' declares source '{}' but that file was not found",
            json_path.display(),
            source_path.display()
        )));
    }

    Ok(Some(source_path))
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
    fn compile_str_errors_on_undeclared_signal_reference() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                "module top(input logic a, output logic y); assign y = missing; endmodule\n",
            )
            .expect_err("missing signal should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(message.contains("missing"), "unexpected message: {message}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_duplicate_declaration_name() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(input logic a, output logic y); ",
                    "logic a; ",
                    "assign y = a; ",
                    "endmodule\n"
                ),
            )
            .expect_err("duplicate declaration should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(message.contains("declares 'a' more than once"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_unsupported_inout_port_direction() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                "module top(inout logic io); endmodule\n",
            )
            .expect_err("inout ports should fail validation");

        match error {
            Error::Unsupported(message) => {
                assert!(
                    message.contains("unsupported `inout` port 'io'"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_continuous_assignment_to_input_port() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(input logic a, output logic y); ",
                    "assign a = 1'b0; ",
                    "assign y = a; ",
                    "endmodule\n"
                ),
            )
            .expect_err("driving an input port should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(
                    message.contains("input port 'a' in 'top' cannot be driven"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_procedural_assignment_to_input_port() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(input logic a, output logic y); ",
                    "always_comb begin ",
                    "a = 1'b0; ",
                    "y = a; ",
                    "end ",
                    "endmodule\n"
                ),
            )
            .expect_err("procedurally driving an input port should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(
                    message.contains("input port 'a' in 'top' cannot be driven"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_unknown_instance_port() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module child(input logic a, output logic y); assign y = a; endmodule\n",
                    "module top(input logic a, output logic y); ",
                    "child u_child (.missing(a), .y(y)); ",
                    "endmodule\n"
                ),
            )
            .expect_err("unknown port connection should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(
                    message.contains("unknown port 'missing'"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_duplicate_instance_port_connection() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module child(input logic a, output logic y); assign y = a; endmodule\n",
                    "module top(input logic a, output logic y); ",
                    "child u_child (.a(a), .a(a), .y(y)); ",
                    "endmodule\n"
                ),
            )
            .expect_err("duplicate port connection should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(message.contains("connects port 'a' more than once"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_output_port_connected_to_input_port() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module child(input logic a, output logic y); assign y = a; endmodule\n",
                    "module top(input logic a, output logic y); ",
                    "child u_child (.a(a), .y(a)); ",
                    "assign y = a; ",
                    "endmodule\n"
                ),
            )
            .expect_err("driving a parent input through a child output should fail validation");

        match error {
            Error::Resolve(message) => {
                assert!(
                    message.contains("input port 'a' in 'top' cannot be driven"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn compile_str_errors_on_output_port_connected_to_non_lvalue() {
        let error = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module child(input logic a, output logic y); assign y = a; endmodule\n",
                    "module top(input logic a, input logic b, output logic y); ",
                    "child u_child (.a(a), .y(a & b)); ",
                    "assign y = a; ",
                    "endmodule\n"
                ),
            )
            .expect_err("non-lvalue output connection should fail validation");

        match error {
            Error::Unsupported(message) => {
                assert!(
                    message.contains("non-lvalue expression"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn run_compile_dir_reports_supported_and_unsupported_files() {
        let temp_dir = unique_temp_dir("compile-dir");
        let nested_dir = temp_dir.join("nested");
        fs::create_dir_all(&nested_dir).expect("create nested dir");

        fs::write(
            temp_dir.join("pass.sv"),
            "module pass(output logic one); assign one = 1'b1; endmodule\n",
        )
        .expect("write pass.sv");

        fs::write(
            nested_dir.join("unsupported.sv"),
            concat!(
                "module unsupported(",
                "input logic clk, input logic d, output logic q",
                "); ",
                "always_latch begin q <= d; end ",
                "endmodule\n"
            ),
        )
        .expect("write unsupported.sv");

        let report = Compiler::new()
            .run_compile_dir(&temp_dir)
            .expect("run compile-only batch");

        assert_eq!(report.total, 2);
        assert_eq!(report.passed, 1);
        assert!(!report.all_passed());
        assert_eq!(
            report
                .files
                .iter()
                .map(|file| {
                    (
                        file.source_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .expect("file name"),
                        file.passed,
                    )
                })
                .collect::<Vec<_>>(),
            vec![("unsupported.sv", false), ("pass.sv", true)],
        );
        assert!(report.files[0].error.is_none());
        assert!(
            report.files[0]
                .diagnostics
                .iter()
                .any(|diag| diag.message.contains("always_latch"))
        );
    }

    #[test]
    fn run_compile_dir_keeps_compile_errors_in_report() {
        let temp_dir = unique_temp_dir("compile-dir-compile-error");

        fs::write(
            temp_dir.join("top.sv"),
            "module top(output logic outY); missing_dep u_missing (.outY(outY)); endmodule\n",
        )
        .expect("write top.sv");

        let report = Compiler::new()
            .run_compile_dir(&temp_dir)
            .expect("run compile-only batch");

        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 0);
        assert!(!report.all_passed());
        assert!(report.files[0].diagnostics.is_empty());
        assert!(
            report.files[0]
                .error
                .as_deref()
                .is_some_and(|message| message.contains("missing_dep"))
        );
    }

    #[test]
    fn run_compile_dirs_aggregates_directory_reports() {
        let temp_dir = unique_temp_dir("compile-dirs");
        let passing_dir = temp_dir.join("passing");
        let failing_dir = temp_dir.join("failing");
        fs::create_dir_all(&passing_dir).expect("create passing dir");
        fs::create_dir_all(&failing_dir).expect("create failing dir");

        fs::write(
            passing_dir.join("pass.sv"),
            "module pass(output logic one); assign one = 1'b1; endmodule\n",
        )
        .expect("write passing suite");

        fs::write(
            failing_dir.join("unsupported.sv"),
            concat!(
                "module unsupported(",
                "input logic clk, input logic d, output logic q",
                "); ",
                "always_latch begin q <= d; end ",
                "endmodule\n"
            ),
        )
        .expect("write failing suite");

        let report = Compiler::new()
            .run_compile_dirs(&[passing_dir.clone(), failing_dir.clone()])
            .expect("run compile-only corpus");

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
    fn run_json_test_dir_discovers_json_only_suite_with_explicit_source() {
        let temp_dir = unique_temp_dir("json-test-dir-explicit-source");

        fs::write(
            temp_dir.join("top.sv"),
            concat!(
                "module top(",
                "input logic clk, input logic reset, output logic outY",
                "); ",
                "always_ff @(posedge clk) begin ",
                "if (reset) outY <= 1'b0; else outY <= 1'b1; ",
                "end ",
                "endmodule\n"
            ),
        )
        .expect("write top.sv");
        fs::write(
            temp_dir.join("top_alias.json"),
            concat!(
                "{",
                "\"source\":\"top.sv\",",
                "\"sequential\":true,",
                "\"test_cases\":[",
                "{",
                "\"sequence\":[",
                "{\"inputs\":{\"clk\":1,\"reset\":1},\"expected\":{\"outY\":0}},",
                "{\"inputs\":{\"clk\":1,\"reset\":0},\"expected\":{\"outY\":1}}",
                "]",
                "}",
                "]",
                "}"
            ),
        )
        .expect("write top_alias.json");

        let report = Compiler::new()
            .run_json_test_dir(&temp_dir)
            .expect("run batch regression");

        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 1);
        assert_eq!(
            report.suites[0]
                .source_path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("top.sv")
        );
        assert_eq!(
            report.suites[0]
                .json_path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("top_alias.json")
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
