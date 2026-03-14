use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use serde::Serialize;
use svsim::{Compiler, HirDesign, JsonTestReport};

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
    #[arg(long = "json-test")]
    json_test: Option<PathBuf>,

    /// SystemVerilog source file to compile.
    file: PathBuf,
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

fn main() -> ExitCode {
    let args = Args::parse();

    let compiler = args
        .search_paths
        .into_iter()
        .fold(Compiler::new(), |compiler, path| {
            compiler.add_search_path(path)
        });

    match compiler.compile_file(&args.file) {
        Ok(design) => {
            if let Some(json_test) = args.json_test {
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
