use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::design::{CompiledDesign, DesignHierarchy, InstanceHierarchy};
use crate::diag::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonTestReport {
    pub passed: usize,
    pub total: usize,
    pub cases: Vec<JsonTestCaseReport>,
}

impl JsonTestReport {
    pub fn all_passed(&self) -> bool {
        self.passed == self.total
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonTestDirectoryReport {
    pub passed: usize,
    pub total: usize,
    pub suites: Vec<JsonTestSuiteRunReport>,
}

impl JsonTestDirectoryReport {
    pub fn all_passed(&self) -> bool {
        self.passed == self.total
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonTestSuiteRunReport {
    pub source_path: PathBuf,
    pub json_path: PathBuf,
    pub top_module: Option<String>,
    pub passed: bool,
    pub report: Option<JsonTestReport>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonTestCaseReport {
    pub name: String,
    pub description: Option<String>,
    pub steps: usize,
    pub passed: bool,
    pub failures: Vec<JsonTestFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonTestFailure {
    pub step: Option<usize>,
    pub signal: String,
    pub expected: u64,
    pub actual: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct JsonTestSuite {
    kind: JsonTestKind,
}

#[derive(Debug, Clone)]
enum JsonTestKind {
    Combinational(Vec<CombinationalTestCase>),
    Sequential(SequentialTestSuite),
}

#[derive(Debug, Clone)]
struct CombinationalTestCase {
    inputs: BTreeMap<String, u64>,
    expected: BTreeMap<String, u64>,
}

#[derive(Debug, Clone)]
struct SequentialTestSuite {
    memory_bindings: Vec<MemoryBinding>,
    cases: Vec<SequentialTestCase>,
}

#[derive(Debug, Clone)]
struct SequentialTestCase {
    name: String,
    description: Option<String>,
    steps: Vec<TestStep>,
}

#[derive(Debug, Clone)]
struct TestStep {
    inputs: BTreeMap<String, u64>,
    expected: BTreeMap<String, u64>,
}

#[derive(Debug, Clone)]
struct MemoryBinding {
    module_name: Option<String>,
    instance_suffix: Option<String>,
    memory_name: Option<String>,
    file: PathBuf,
}

#[derive(Debug, Clone)]
struct MatchedInstance {
    module_name: String,
    path: Vec<String>,
}

impl JsonTestSuite {
    pub fn load_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)?;
        Self::from_str_with_path(path, &text)
    }

    pub fn run(&self, design: &CompiledDesign) -> Result<JsonTestReport> {
        match &self.kind {
            JsonTestKind::Combinational(cases) => self.run_combinational(design, cases),
            JsonTestKind::Sequential(suite) => self.run_sequential(design, suite),
        }
    }

    fn from_str_with_path(path: &Path, text: &str) -> Result<Self> {
        let raw = serde_json::from_str::<RawJsonTestSuite>(text).map_err(|error| {
            Error::Parse(format!(
                "failed to parse JSON test file {}: {error}",
                path.display()
            ))
        })?;

        let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let kind = match raw {
            RawJsonTestSuite::Combinational(cases) => JsonTestKind::Combinational(
                cases
                    .into_iter()
                    .map(|case| CombinationalTestCase {
                        inputs: case.inputs,
                        expected: case.expect,
                    })
                    .collect(),
            ),
            RawJsonTestSuite::Sequential(raw_suite) => {
                JsonTestKind::Sequential(SequentialTestSuite::from_raw(raw_suite, base_dir))
            }
        };

        Ok(Self { kind })
    }

    fn run_combinational(
        &self,
        design: &CompiledDesign,
        cases: &[CombinationalTestCase],
    ) -> Result<JsonTestReport> {
        let mut sim = design.instantiate_top()?;
        let mut results = Vec::with_capacity(cases.len());

        for (index, case) in cases.iter().enumerate() {
            let actual = sim.eval_once(case.inputs.clone())?;
            let failures = compare_outputs(&actual, &case.expected, None);
            results.push(JsonTestCaseReport {
                name: format!("Test {}", index + 1),
                description: None,
                steps: 1,
                passed: failures.is_empty(),
                failures,
            });
        }

        Ok(build_report(results))
    }

    fn run_sequential(
        &self,
        design: &CompiledDesign,
        suite: &SequentialTestSuite,
    ) -> Result<JsonTestReport> {
        let mut sim = design.instantiate_top()?;
        apply_memory_bindings(design, &mut sim, &suite.memory_bindings)?;

        let mut results = Vec::with_capacity(suite.cases.len());
        for case in &suite.cases {
            let mut failures = Vec::new();

            for (step_index, step) in case.steps.iter().enumerate() {
                let actual = sim.step(step.inputs.clone())?;
                failures.extend(compare_outputs(
                    &actual,
                    &step.expected,
                    Some(step_index + 1),
                ));
            }

            results.push(JsonTestCaseReport {
                name: case.name.clone(),
                description: case.description.clone(),
                steps: case.steps.len(),
                passed: failures.is_empty(),
                failures,
            });
        }

        Ok(build_report(results))
    }
}

impl SequentialTestSuite {
    fn from_raw(raw: RawSequentialTestSuite, base_dir: &Path) -> Self {
        let mut memory_bindings = Vec::new();
        memory_bindings.extend(normalize_memory_entries(
            raw.memory_init.unwrap_or_default(),
            base_dir,
        ));

        if let Some(memory_files) = raw.memory_files {
            if let Some(entries) = memory_files.rom {
                memory_bindings.extend(normalize_memory_entries(entries.into_vec(), base_dir));
            }
            if let Some(entries) = memory_files.ram {
                memory_bindings.extend(normalize_memory_entries(entries.into_vec(), base_dir));
            }
        }

        if let Some(entries) = raw.rom {
            memory_bindings.extend(normalize_memory_entries(entries.into_vec(), base_dir));
        }
        if let Some(entries) = raw.ram {
            memory_bindings.extend(normalize_memory_entries(entries.into_vec(), base_dir));
        }

        let cases = raw
            .test_cases
            .into_iter()
            .enumerate()
            .map(|(index, case)| SequentialTestCase {
                name: case.name.unwrap_or_else(|| format!("Test {}", index + 1)),
                description: case.description,
                steps: if let Some(sequence) = case.sequence {
                    sequence
                        .into_iter()
                        .map(|step| TestStep {
                            inputs: step.inputs,
                            expected: step.expected,
                        })
                        .collect()
                } else {
                    vec![TestStep {
                        inputs: case.inputs.unwrap_or_default(),
                        expected: case.expected.unwrap_or_default(),
                    }]
                },
            })
            .collect();

        Self {
            memory_bindings,
            cases,
        }
    }
}

fn normalize_memory_entries(entries: Vec<RawMemoryEntry>, base_dir: &Path) -> Vec<MemoryBinding> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let file = entry.file.or(entry.path)?;
            let file = if Path::new(&file).is_absolute() {
                PathBuf::from(file)
            } else {
                base_dir.join(file)
            };

            Some(MemoryBinding {
                module_name: entry.module.filter(|value| !value.is_empty()),
                instance_suffix: entry
                    .instance
                    .or(entry.instance_path)
                    .filter(|value| !value.is_empty()),
                memory_name: entry
                    .memory
                    .or(entry.name)
                    .filter(|value| !value.is_empty()),
                file,
            })
        })
        .collect()
}

fn apply_memory_bindings(
    design: &CompiledDesign,
    sim: &mut crate::sim::SimulationSession,
    bindings: &[MemoryBinding],
) -> Result<()> {
    if bindings.is_empty() {
        return Ok(());
    }

    let hierarchy = design.hierarchy()?;
    for binding in bindings {
        let matches = matching_instances(
            &hierarchy,
            binding.module_name.as_deref(),
            binding.instance_suffix.as_deref(),
        );

        if matches.is_empty() {
            return Err(Error::Resolve(format!(
                "memory binding did not match any instances for module {:?} and instance {:?}",
                binding.module_name, binding.instance_suffix
            )));
        }

        for matched in matches {
            let memory_names = if let Some(memory_name) = binding.memory_name.as_deref() {
                vec![memory_name.to_owned()]
            } else {
                let module = design.hir().module(&matched.module_name).ok_or_else(|| {
                    Error::Resolve(format!(
                        "module '{}' is missing from the compiled design",
                        matched.module_name
                    ))
                })?;
                module
                    .memories
                    .iter()
                    .map(|memory| memory.name.clone())
                    .collect()
            };

            if memory_names.is_empty() {
                return Err(Error::Resolve(format!(
                    "memory binding for module '{}' matched an instance with no memories",
                    matched.module_name
                )));
            }

            let path_refs = matched.path.iter().map(String::as_str).collect::<Vec<_>>();
            for memory_name in memory_names {
                sim.load_memory_file(&path_refs, &memory_name, &binding.file)?;
            }
        }
    }

    Ok(())
}

fn matching_instances(
    hierarchy: &DesignHierarchy,
    module_name: Option<&str>,
    instance_suffix: Option<&str>,
) -> Vec<MatchedInstance> {
    let mut matches = Vec::new();
    let mut path = Vec::new();
    collect_matching_instances(
        &hierarchy.top_module,
        &hierarchy.children,
        &mut path,
        module_name,
        instance_suffix,
        &mut matches,
    );
    matches
}

fn collect_matching_instances(
    current_module_name: &str,
    children: &[InstanceHierarchy],
    path: &mut Vec<String>,
    module_name: Option<&str>,
    instance_suffix: Option<&str>,
    matches: &mut Vec<MatchedInstance>,
) {
    let path_string = path.join(".");
    let module_matches = module_name.is_none_or(|name| name == current_module_name);
    let instance_matches = instance_suffix
        .is_none_or(|suffix| path_string == suffix || path_string.ends_with(&format!(".{suffix}")));

    if module_matches && instance_matches {
        matches.push(MatchedInstance {
            module_name: current_module_name.to_owned(),
            path: path.clone(),
        });
    }

    for child in children {
        path.push(child.instance_name.clone());
        collect_matching_instances(
            &child.module_name,
            &child.children,
            path,
            module_name,
            instance_suffix,
            matches,
        );
        path.pop();
    }
}

fn compare_outputs(
    actual: &BTreeMap<String, u64>,
    expected: &BTreeMap<String, u64>,
    step: Option<usize>,
) -> Vec<JsonTestFailure> {
    let mut failures = Vec::new();
    for (signal, expected_value) in expected {
        let actual_value = actual.get(signal).copied();
        if actual_value != Some(*expected_value) {
            failures.push(JsonTestFailure {
                step,
                signal: signal.clone(),
                expected: *expected_value,
                actual: actual_value,
            });
        }
    }
    failures
}

fn build_report(cases: Vec<JsonTestCaseReport>) -> JsonTestReport {
    let passed = cases.iter().filter(|case| case.passed).count();
    let total = cases.len();
    JsonTestReport {
        passed,
        total,
        cases,
    }
}

pub(crate) fn build_directory_report(
    suites: Vec<JsonTestSuiteRunReport>,
) -> JsonTestDirectoryReport {
    let passed = suites.iter().filter(|suite| suite.passed).count();
    let total = suites.len();
    JsonTestDirectoryReport {
        passed,
        total,
        suites,
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawJsonTestSuite {
    Combinational(Vec<RawCombinationalTestCase>),
    Sequential(RawSequentialTestSuite),
}

#[derive(Debug, Deserialize)]
struct RawCombinationalTestCase {
    #[serde(default)]
    expect: BTreeMap<String, u64>,
    #[serde(flatten)]
    inputs: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSequentialTestSuite {
    #[serde(default)]
    test_cases: Vec<RawSequentialTestCase>,
    #[serde(default)]
    memory_files: Option<RawMemoryFileGroups>,
    #[serde(default)]
    memory_init: Option<Vec<RawMemoryEntry>>,
    #[serde(default)]
    rom: Option<OneOrMany<RawMemoryEntry>>,
    #[serde(default)]
    ram: Option<OneOrMany<RawMemoryEntry>>,
}

#[derive(Debug, Deserialize, Default)]
struct RawMemoryFileGroups {
    #[serde(default)]
    rom: Option<OneOrMany<RawMemoryEntry>>,
    #[serde(default)]
    ram: Option<OneOrMany<RawMemoryEntry>>,
}

#[derive(Debug, Deserialize)]
struct RawSequentialTestCase {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    inputs: Option<BTreeMap<String, u64>>,
    #[serde(default)]
    expected: Option<BTreeMap<String, u64>>,
    #[serde(default)]
    sequence: Option<Vec<RawTestStep>>,
}

#[derive(Debug, Deserialize)]
struct RawTestStep {
    #[serde(default)]
    inputs: BTreeMap<String, u64>,
    #[serde(default)]
    expected: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct RawMemoryEntry {
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    instance: Option<String>,
    #[serde(default)]
    instance_path: Option<String>,
    #[serde(default)]
    memory: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::Compiler;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn run_json_file_passes_combinational_corpus_suite() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/full_adder.sv"))
            .expect("compile full_adder");

        let report = design
            .run_json_file(repo.join("parts/basic/full_adder.json"))
            .expect("run json tests");

        assert!(report.all_passed());
        assert_eq!(report.passed, report.total);
    }

    #[test]
    fn run_json_file_passes_sequential_corpus_suite() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/register_8bit.sv"))
            .expect("compile register_8bit");

        let report = design
            .run_json_file(repo.join("parts/basic/register_8bit.json"))
            .expect("run json tests");

        assert!(report.all_passed());
        assert_eq!(report.passed, report.total);
    }

    #[test]
    fn run_json_file_preloads_memory_backed_suite() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/memory_cpu_stub.sv"))
            .expect("compile memory_cpu_stub");

        let report = design
            .run_json_file(repo.join("parts/testing/memory_cpu_stub.json"))
            .expect("run json tests");

        assert!(report.all_passed());
        assert_eq!(report.passed, report.total);
    }

    #[test]
    fn run_json_file_reports_output_mismatch() {
        let temp_dir = unique_temp_dir("json-test-mismatch");
        let design = Compiler::new()
            .compile_str(
                temp_dir.join("top.sv"),
                "module top(output logic one); assign one = 1'b1; endmodule\n",
            )
            .expect("compile top");
        let json_path = temp_dir.join("top.json");
        fs::write(&json_path, "[{\"expect\":{\"one\":0}}]").expect("write json");

        let report = design.run_json_file(&json_path).expect("run json tests");

        assert!(!report.all_passed());
        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 0);
        assert_eq!(report.cases[0].failures.len(), 1);
        assert_eq!(report.cases[0].failures[0].signal, "one");
        assert_eq!(report.cases[0].failures[0].actual, Some(1));
        assert_eq!(report.cases[0].failures[0].expected, 0);
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("svsim-{name}-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
