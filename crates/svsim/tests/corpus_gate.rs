//! Corpus gate: `cargo test` fails unless every JSON regression suite under
//! the green `parts/` directories passes, and unless every suite in the
//! intentional negative corpus (`parts/failing`) still fails with its
//! expected diagnostic (`corpus_failing_stays_red`).
//!
//! `parts/roms` holds data assets only and is deliberately excluded.
//! `run_json_test_dir` errors when a directory contains no
//! SystemVerilog/JSON pairs, so a moved or emptied corpus directory fails
//! the gate instead of passing vacuously.

use std::path::PathBuf;

use svsim::Compiler;

fn parts_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../parts")
        .join(name)
        .canonicalize()
        .unwrap_or_else(|error| panic!("resolve parts/{name}: {error}"))
}

fn assert_corpus_green(name: &str) {
    let report = Compiler::new()
        .run_json_test_dir(parts_dir(name))
        .unwrap_or_else(|error| panic!("run parts/{name} regression suites: {error}"));

    if report.all_passed() {
        return;
    }

    let mut details = String::new();
    for suite in report.suites.iter().filter(|suite| !suite.passed) {
        details.push_str(&format!("\n  suite {}:", suite.json_path.display()));
        if let Some(error) = &suite.error {
            details.push_str(&format!(" {error}"));
        }
        if let Some(suite_report) = &suite.report {
            for case in suite_report.cases.iter().filter(|case| !case.passed) {
                details.push_str(&format!("\n    case '{}':", case.name));
                for failure in &case.failures {
                    details.push_str(&format!(
                        "\n      step {:?}: {} expected {:?}, got {:?}",
                        failure.step, failure.signal, failure.expected, failure.actual
                    ));
                }
            }
        }
    }
    panic!(
        "parts/{name} is not green: {}/{} cases passed{details}",
        report.passed, report.total
    );
}

#[test]
fn corpus_basic_is_green() {
    assert_corpus_green("basic");
}

#[test]
fn corpus_overture_is_green() {
    assert_corpus_green("overture");
}

#[test]
fn corpus_picorv32_is_green() {
    assert_corpus_green("picorv32");
}

#[test]
fn corpus_rv32i_is_green() {
    assert_corpus_green("rv32i");
}

#[test]
fn corpus_sap1_is_green() {
    assert_corpus_green("sap1");
}

#[test]
fn corpus_sap2_is_green() {
    assert_corpus_green("sap2");
}

#[test]
fn corpus_sap3_is_green() {
    assert_corpus_green("sap3");
}

#[test]
fn corpus_simple8_is_green() {
    assert_corpus_green("simple8");
}

#[test]
fn corpus_testing_is_green() {
    assert_corpus_green("testing");
}

/// Every suite here must keep failing with a suite-level error containing
/// the given fragment. Fragments avoid absolute paths and parser-internal
/// positions so the assertions survive unrelated churn.
const FAILING_SUITES_WITH_ERROR: &[(&str, &str)] = &[
    (
        "constant_memory_index_oob.json",
        "memory index [2] is out of range for 'rom'",
    ),
    (
        "duplicate_instance_names.json",
        "declares instance 'u_dup' more than once",
    ),
    ("malformed_json.json", "failed to parse JSON test file"),
    (
        "missing_child_module.json",
        "module 'missing_dependency' was not found",
    ),
    ("syntax_error.json", "failed to parse"),
];

/// Suites that compile and run but must keep failing on an expectation
/// mismatch for the given signal.
const FAILING_SUITES_WITH_MISMATCH: &[(&str, &str)] = &[("constant_one_mismatch.json", "outY")];

#[test]
fn corpus_failing_stays_red() {
    let report = Compiler::new()
        .run_json_test_dir(parts_dir("failing"))
        .unwrap_or_else(|error| panic!("run parts/failing regression suites: {error}"));

    let expected_count = FAILING_SUITES_WITH_ERROR.len() + FAILING_SUITES_WITH_MISMATCH.len();
    let suite_names: Vec<String> = report
        .suites
        .iter()
        .filter_map(|suite| suite.json_path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        report.suites.len(),
        expected_count,
        "parts/failing suite set changed; update corpus_gate.rs to cover it: {suite_names:?}"
    );

    for suite in &report.suites {
        assert!(
            !suite.passed,
            "negative suite {} unexpectedly passed; parts/failing must stay red",
            suite.json_path.display()
        );
    }

    let find_suite = |name: &str| {
        report
            .suites
            .iter()
            .find(|suite| suite.json_path.file_name().is_some_and(|file| file == name))
            .unwrap_or_else(|| panic!("negative suite {name} is missing from parts/failing"))
    };

    for (name, fragment) in FAILING_SUITES_WITH_ERROR {
        let suite = find_suite(name);
        let error = suite
            .error
            .as_deref()
            .unwrap_or_else(|| panic!("negative suite {name} has no suite-level error"));
        assert!(
            error.contains(fragment),
            "negative suite {name} error drifted: expected fragment {fragment:?}, got {error:?}"
        );
    }

    for (name, signal) in FAILING_SUITES_WITH_MISMATCH {
        let suite = find_suite(name);
        let suite_report = suite
            .report
            .as_ref()
            .unwrap_or_else(|| panic!("negative suite {name} produced no case report"));
        let mismatched = suite_report.cases.iter().any(|case| {
            case.failures
                .iter()
                .any(|failure| failure.signal == *signal)
        });
        assert!(
            mismatched,
            "negative suite {name} no longer reports an expectation mismatch on '{signal}'"
        );
    }
}
