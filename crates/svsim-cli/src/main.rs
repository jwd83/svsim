use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use serde::Serialize;
use svsim::{Compiler, HirDesign, JsonTestCorpusReport, JsonTestDirectoryReport, JsonTestReport};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "SystemVerilog parser and simulator workspace CLI"
)]
struct Args {
    /// Add a SystemVerilog include or module search path.
    #[arg(short = 'I', long = "search-path")]
    search_paths: Vec<PathBuf>,

    /// Run a JSON regression suite against the compiled design.
    #[arg(
        long = "json-test",
        conflicts_with = "json_test_dirs",
        requires = "file"
    )]
    json_test: Option<PathBuf>,

    /// Run all sibling *.sv/*.json regression pairs under a directory.
    #[arg(long = "json-test-dir", conflicts_with = "json_test")]
    json_test_dirs: Vec<PathBuf>,

    /// SystemVerilog source file to compile.
    #[arg(required_unless_present = "json_test_dirs")]
    file: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct ParseOutput<'a> {
    top_module: Option<&'a str>,
    hir: &'a HirDesign,
}

#[derive(Debug, Serialize)]
struct TestOutput<'a> {
    top_module: Option<&'a str>,
    report: JsonTestReport,
}

#[derive(Debug, Serialize)]
struct BatchTestOutput {
    directory: PathBuf,
    report: JsonTestDirectoryReport,
}

#[derive(Debug, Serialize)]
struct CorpusTestOutput {
    report: JsonTestCorpusReport,
}

fn main() -> ExitCode {
    let Args {
        search_paths,
        json_test,
        json_test_dirs,
        file,
    } = Args::parse();

    let compiler = search_paths
        .into_iter()
        .fold(Compiler::new(), |compiler, path| {
            compiler.add_search_path(path)
        });

    if !json_test_dirs.is_empty() {
        if json_test_dirs.len() == 1 {
            let json_test_dir = json_test_dirs.into_iter().next().expect("one directory");
            match compiler.run_json_test_dir(&json_test_dir) {
                Ok(report) => {
                    let all_passed = report.all_passed();
                    let output = BatchTestOutput {
                        directory: json_test_dir,
                        report,
                    };
                    if write_json(&output).is_err() {
                        ExitCode::FAILURE
                    } else if all_passed {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        } else {
            match compiler.run_json_test_dirs(&json_test_dirs) {
                Ok(report) => {
                    let all_passed = report.all_passed();
                    let output = CorpusTestOutput { report };
                    if write_json(&output).is_err() {
                        ExitCode::FAILURE
                    } else if all_passed {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
    } else {
        let file = file.expect("clap should require a file unless --json-test-dir is used");
        match compiler.compile_file(&file) {
            Ok(design) => {
                if let Some(json_test) = json_test {
                    match design.run_json_file(&json_test) {
                        Ok(report) => {
                            let all_passed = report.all_passed();
                            let output = TestOutput {
                                top_module: design.top_module(),
                                report,
                            };
                            if write_json(&output).is_err() {
                                ExitCode::FAILURE
                            } else if all_passed {
                                ExitCode::SUCCESS
                            } else {
                                ExitCode::FAILURE
                            }
                        }
                        Err(error) => {
                            eprintln!("{error}");
                            ExitCode::FAILURE
                        }
                    }
                } else {
                    let output = ParseOutput {
                        top_module: design.top_module(),
                        hir: design.hir(),
                    };

                    if write_json(&output).is_err() {
                        ExitCode::FAILURE
                    } else {
                        ExitCode::SUCCESS
                    }
                }
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        }
    }
}

fn write_json<T: Serialize>(value: &T) -> Result<(), ()> {
    match serde_json::to_writer_pretty(std::io::stdout(), value) {
        Ok(()) => {
            println!();
            Ok(())
        }
        Err(error) => {
            eprintln!("failed to write JSON output: {error}");
            Err(())
        }
    }
}
