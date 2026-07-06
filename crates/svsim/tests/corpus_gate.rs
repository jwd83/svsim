//! Green-corpus gate: `cargo test` fails unless every JSON regression suite
//! under the green `parts/` directories passes.
//!
//! `parts/failing` is the intentional negative corpus and `parts/roms` holds
//! data assets only; both are deliberately excluded. `run_json_test_dir`
//! errors when a directory contains no SystemVerilog/JSON pairs, so a moved
//! or emptied corpus directory fails the gate instead of passing vacuously.

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
