use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use serde::Serialize;
use svsim::{Compiler, HirDesign};

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

    /// SystemVerilog source file to compile.
    file: PathBuf,
}

#[derive(Debug, Serialize)]
struct ParseOutput<'a> {
    top_module: Option<&'a str>,
    hir: &'a HirDesign,
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
            let output = ParseOutput {
                top_module: design.top_module(),
                hir: design.hir(),
            };

            match serde_json::to_writer_pretty(std::io::stdout(), &output) {
                Ok(()) => {
                    println!();
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("failed to write JSON output: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
