use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, process};

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

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

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

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["top_module"], "full_adder");
    assert!(json["report"]["duration_ms"].is_u64());
    assert!(json["report"]["step_hz"].is_u64());
    assert_eq!(json["report"]["passed"], 8);
    assert_eq!(json["report"]["total"], 8);
}

#[test]
fn cli_reports_four_state_values_in_json_regression_output() {
    let temp_dir = unique_temp_dir("cli-json-four-state");
    fs::write(
        temp_dir.join("top.sv"),
        concat!(
            "module top(",
            "input logic inA, ",
            "output logic outY",
            "); ",
            "assign outY = inA; ",
            "endmodule\n"
        ),
    )
    .expect("write top.sv");
    fs::write(
        temp_dir.join("top.json"),
        "[{\"inA\":\"x\",\"expect\":{\"outY\":0}}]",
    )
    .expect("write top.json");

    let output = Command::new(svsim_bin())
        .arg("--json-test")
        .arg(temp_dir.join("top.json"))
        .arg(temp_dir.join("top.sv"))
        .output()
        .expect("run svsim");

    assert!(
        !output.status.success(),
        "expected mismatch report, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["report"]["passed"], 0);
    assert_eq!(json["report"]["total"], 1);
    assert_eq!(
        json["report"]["cases"][0]["failures"][0]["actual"],
        Value::String("x".into())
    );
}

#[test]
fn cli_runs_json_regression_directory() {
    let temp_dir = unique_temp_dir("cli-json-test-dir");
    fs::write(
        temp_dir.join("pass.sv"),
        "module pass(output logic one); assign one = 1'b1; endmodule\n",
    )
    .expect("write pass.sv");
    fs::write(temp_dir.join("pass.json"), "[{\"expect\":{\"one\":1}}]").expect("write pass.json");

    let output = Command::new(svsim_bin())
        .arg("--json-test-dir")
        .arg(&temp_dir)
        .output()
        .expect("run svsim");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert!(json["report"]["duration_ms"].is_u64());
    assert_eq!(json["report"]["passed"], 1);
    assert_eq!(json["report"]["total"], 1);
    assert!(json["report"]["suites"][0]["duration_ms"].is_u64());
    assert!(json["report"]["suites"][0]["report"]["duration_ms"].is_u64());
    assert!(json["report"]["suites"][0]["report"]["step_hz"].is_u64());
    assert_eq!(json["report"]["suites"][0]["top_module"], "pass");
}

#[test]
fn cli_runs_sap2_json_regression_directory() {
    let repo = repo_root();
    let output = Command::new(svsim_bin())
        .arg("--json-test-dir")
        .arg(repo.join("parts/sap2"))
        .output()
        .expect("run svsim");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["report"]["passed"], json["report"]["total"]);
    assert!(
        json["report"]["total"].as_u64().is_some_and(|total| total >= 2),
        "unexpected sap2 suite count: {}",
        json["report"]["total"]
    );
}

#[test]
fn cli_runs_compile_directory() {
    let temp_dir = unique_temp_dir("cli-compile-dir");
    fs::write(
        temp_dir.join("pass.sv"),
        "module pass(output logic one); assign one = 1'b1; endmodule\n",
    )
    .expect("write pass.sv");
    fs::write(
        temp_dir.join("verilog_pass.v"),
        "module verilog_pass(output wire one); assign one = 1'b1; endmodule\n",
    )
    .expect("write verilog_pass.v");

    let output = Command::new(svsim_bin())
        .arg("--compile-dir")
        .arg(&temp_dir)
        .output()
        .expect("run svsim");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert!(json["report"]["duration_ms"].is_u64());
    assert_eq!(json["report"]["passed"], 2);
    assert_eq!(json["report"]["total"], 2);
    assert_eq!(json["report"]["files"][0]["top_module"], "pass");
    assert_eq!(json["report"]["files"][0]["module_count"], 1);
    assert_eq!(
        json["report"]["files"][0]["diagnostics"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(json["report"]["files"][1]["top_module"], "verilog_pass");
    assert_eq!(json["report"]["files"][1]["module_count"], 1);
    assert_eq!(
        json["report"]["files"][1]["diagnostics"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}

#[test]
fn cli_runs_multiple_compile_directories() {
    let temp_dir = unique_temp_dir("cli-compile-dirs");
    let left_dir = temp_dir.join("left");
    let right_dir = temp_dir.join("right");
    fs::create_dir_all(&left_dir).expect("create left dir");
    fs::create_dir_all(&right_dir).expect("create right dir");

    fs::write(
        left_dir.join("left_pass.sv"),
        "module left_pass(output logic one); assign one = 1'b1; endmodule\n",
    )
    .expect("write left_pass.sv");

    fs::write(
        right_dir.join("right_pass.sv"),
        "module right_pass(output logic two); assign two = 1'b1; endmodule\n",
    )
    .expect("write right_pass.sv");

    let output = Command::new(svsim_bin())
        .arg("--compile-dir")
        .arg(&left_dir)
        .arg("--compile-dir")
        .arg(&right_dir)
        .output()
        .expect("run svsim");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert!(json["report"]["duration_ms"].is_u64());
    assert_eq!(json["report"]["passed"], 2);
    assert_eq!(json["report"]["total"], 2);
    assert_eq!(
        json["report"]["directories"][0]["directory"],
        left_dir.display().to_string()
    );
    assert_eq!(json["report"]["directories"][0]["report"]["passed"], 1);
    assert_eq!(
        json["report"]["directories"][1]["directory"],
        right_dir.display().to_string()
    );
    assert_eq!(json["report"]["directories"][1]["report"]["passed"], 1);
}

#[test]
fn cli_runs_multiple_json_regression_directories() {
    let temp_dir = unique_temp_dir("cli-json-test-dirs");
    let left_dir = temp_dir.join("left");
    let right_dir = temp_dir.join("right");
    fs::create_dir_all(&left_dir).expect("create left dir");
    fs::create_dir_all(&right_dir).expect("create right dir");

    fs::write(
        left_dir.join("left_pass.sv"),
        "module left_pass(output logic one); assign one = 1'b1; endmodule\n",
    )
    .expect("write left_pass.sv");
    fs::write(
        left_dir.join("left_pass.json"),
        "[{\"expect\":{\"one\":1}}]",
    )
    .expect("write left_pass.json");

    fs::write(
        right_dir.join("right_pass.sv"),
        "module right_pass(output logic two); assign two = 1'b1; endmodule\n",
    )
    .expect("write right_pass.sv");
    fs::write(
        right_dir.join("right_pass.json"),
        "[{\"expect\":{\"two\":1}}]",
    )
    .expect("write right_pass.json");

    let output = Command::new(svsim_bin())
        .arg("--json-test-dir")
        .arg(&left_dir)
        .arg("--json-test-dir")
        .arg(&right_dir)
        .output()
        .expect("run svsim");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert!(json["report"]["duration_ms"].is_u64());
    assert_eq!(json["report"]["passed"], 2);
    assert_eq!(json["report"]["total"], 2);
    assert_eq!(
        json["report"]["directories"][0]["directory"],
        left_dir.display().to_string()
    );
    assert!(json["report"]["directories"][0]["report"]["duration_ms"].is_u64());
    assert_eq!(json["report"]["directories"][0]["report"]["passed"], 1);
    assert_eq!(
        json["report"]["directories"][1]["directory"],
        right_dir.display().to_string()
    );
    assert!(json["report"]["directories"][1]["report"]["duration_ms"].is_u64());
    assert_eq!(json["report"]["directories"][1]["report"]["passed"], 1);
}

#[test]
fn cli_runs_json_regression_directory_with_explicit_source_suite() {
    let temp_dir = unique_temp_dir("cli-json-test-dir-explicit-source");
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

    let output = Command::new(svsim_bin())
        .arg("--json-test-dir")
        .arg(&temp_dir)
        .output()
        .expect("run svsim");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["report"]["passed"], 1);
    assert_eq!(json["report"]["total"], 1);
    assert_eq!(
        json["report"]["suites"][0]["source_path"],
        temp_dir.join("top.sv").display().to_string()
    );
    assert_eq!(
        json["report"]["suites"][0]["json_path"],
        temp_dir.join("top_alias.json").display().to_string()
    );
}

#[test]
fn cli_reports_expected_failures_for_failing_corpus() {
    let repo = repo_root();
    let output = Command::new(svsim_bin())
        .arg("--json-test-dir")
        .arg(repo.join("parts/failing"))
        .output()
        .expect("run svsim");

    assert!(
        !output.status.success(),
        "expected failing corpus to return failure, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("parse stdout json");
    assert_eq!(json["report"]["passed"], 0);
    assert_eq!(json["report"]["total"], 6);

    let suites = json["report"]["suites"].as_array().expect("suite array");
    assert_eq!(suites.len(), 6);

    let constant_one_mismatch = suites
        .iter()
        .find(|suite| {
            suite["source_path"]
                == repo
                    .join("parts/failing/constant_one_mismatch.sv")
                    .display()
                    .to_string()
        })
        .expect("constant_one_mismatch suite");
    assert_eq!(constant_one_mismatch["passed"], false);
    assert_eq!(constant_one_mismatch["report"]["passed"], 0);
    assert_eq!(constant_one_mismatch["report"]["total"], 1);

    let duplicate_instance_names = suites
        .iter()
        .find(|suite| {
            suite["source_path"]
                == repo
                    .join("parts/failing/duplicate_instance_names.sv")
                    .display()
                    .to_string()
        })
        .expect("duplicate_instance_names suite");
    assert_eq!(duplicate_instance_names["passed"], false);
    assert!(duplicate_instance_names["report"].is_null());
    assert!(
        duplicate_instance_names["error"]
            .as_str()
            .is_some_and(|message| message.contains("more than once"))
    );

    let constant_memory_index_oob = suites
        .iter()
        .find(|suite| {
            suite["source_path"]
                == repo
                    .join("parts/failing/constant_memory_index_oob.sv")
                    .display()
                    .to_string()
        })
        .expect("constant_memory_index_oob suite");
    assert_eq!(constant_memory_index_oob["passed"], false);
    assert!(constant_memory_index_oob["report"].is_null());
    assert!(
        constant_memory_index_oob["error"]
            .as_str()
            .is_some_and(|message| message.contains("memory index [2] is out of range"))
    );

    let malformed_json = suites
        .iter()
        .find(|suite| {
            suite["source_path"]
                == repo
                    .join("parts/failing/malformed_json.sv")
                    .display()
                    .to_string()
        })
        .expect("malformed_json suite");
    assert_eq!(malformed_json["passed"], false);
    assert!(malformed_json["report"].is_null());
    assert!(
        malformed_json["error"]
            .as_str()
            .is_some_and(|message| message.contains("failed to parse JSON test file"))
    );

    let missing_child_module = suites
        .iter()
        .find(|suite| {
            suite["source_path"]
                == repo
                    .join("parts/failing/missing_child_module.sv")
                    .display()
                    .to_string()
        })
        .expect("missing_child_module suite");
    assert_eq!(missing_child_module["passed"], false);
    assert!(missing_child_module["report"].is_null());
    assert!(
        missing_child_module["error"]
            .as_str()
            .is_some_and(|message| message.contains("missing_dependency"))
    );

    let syntax_error = suites
        .iter()
        .find(|suite| {
            suite["source_path"]
                == repo
                    .join("parts/failing/syntax_error.sv")
                    .display()
                    .to_string()
        })
        .expect("syntax_error suite");
    assert_eq!(syntax_error["passed"], false);
    assert!(syntax_error["report"].is_null());
    assert!(
        syntax_error["error"]
            .as_str()
            .is_some_and(|message| message.contains("parse"))
    );
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("svsim-{name}-{}-{nonce}", process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
