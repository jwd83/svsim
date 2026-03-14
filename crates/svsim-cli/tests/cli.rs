use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn svsim_bin() -> &'static str {
    env!("CARGO_BIN_EXE_svsim")
}

#[test]
fn cli_emits_hir_json_for_parse_mode() {
    let repo = repo_root();
    let output = Command::new(svsim_bin())
        .arg(repo.join("parts/basic/full_adder.sv"))
        .output()
        .expect("run svsim");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["top_module"], "full_adder");
    assert_eq!(json["hir"]["files"][0]["modules"][0]["name"], "full_adder");
}

#[test]
fn cli_runs_json_regression_suite() {
    let repo = repo_root();
    let output = Command::new(svsim_bin())
        .arg("--json-test")
        .arg(repo.join("parts/basic/full_adder.json"))
        .arg(repo.join("parts/basic/full_adder.sv"))
        .output()
        .expect("run svsim");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["top_module"], "full_adder");
    assert_eq!(json["report"]["passed"], 8);
    assert_eq!(json["report"]["total"], 8);
}
