# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust rewrite of a Python SystemVerilog simulator. The
Python reference implementation (formerly vendored under `ref/`) is no longer
part of this checkout; the compatibility corpus under `parts/` is the behavior
oracle.

- `crates/svsim/`: core library crate. Keep compiler, frontend, HIR, diagnostics, and simulation logic here.
- `crates/svsim-cli/`: thin CLI wrapper over `svsim`.
- `crates/svsim-render/`: deferred rendering crate for truth-table and waveform output.
- `parts/basic/`, `parts/testing/`, `parts/overture/`, `parts/rv32i/`, `parts/picorv32/`, `parts/sap1/`, `parts/sap2/`, `parts/sap3/`, `parts/simple8/`: SystemVerilog corpus and JSON expectations forming the all-green compatibility suite, enforced by `crates/svsim/tests/corpus_gate.rs`.
- `parts/failing/`: intentionally failing negative corpus, gated as must-fail by `corpus_failing_stays_red` in `crates/svsim/tests/corpus_gate.rs`.
- `docs/`: architecture notes, port compromises, progress reports, and generated corpus reports (`docs/tests/`).
- `plans/`: plan lifecycle (`in-progress/`, `completed/`).
- `wiki/`: maintained knowledge base (architecture, status, testing, roadmap).

Prefer adding new simulator code under `crates/svsim/src/` before splitting into more crates.

## Build, Test, and Development Commands

- `cargo fmt`: format the Rust workspace.
- `cargo test`: build and run all Rust tests, including the green-corpus gate (`crates/svsim/tests/corpus_gate.rs`). This is the gating check.
- `cargo test -p svsim`: run core library tests plus the corpus gate.
- `./test.sh`: regenerate the committed corpus reports under `docs/tests/` (release build; not the gate).
- `cargo run -p svsim-cli -- parts/basic/full_adder.sv`: parse a real module and emit JSON.
- `cargo run -p svsim-cli -- --json-test parts/basic/full_adder.json parts/basic/full_adder.sv`: run a JSON regression suite through the Rust CLI and emit a structured report.

Note: the workspace `Cargo.toml` builds the `svsim` package with `opt-level = 2`
even in dev/test profiles — unoptimized simulation is ~9x slower and makes the
corpus gate impractical.

When adding support for new language features, verify them against files under `parts/` instead of isolated toy inputs only.
Keep `parts/failing/` out of "green" corpus expectations unless you are explicitly checking failure reporting.

## Coding Style & Naming Conventions

Use 4-space indentation and standard Rust naming:

- `snake_case` for modules, files, and functions
- `CamelCase` for types
- `SCREAMING_SNAKE_CASE` for constants

Keep public APIs small and explicit. `sv-parser` types should stay inside the frontend layer; lower them into owned HIR types before passing data deeper into the system. Prefer explicit configuration for memory/program bindings over hidden naming conventions.

## Testing Guidelines

Use Rust unit tests alongside the code they cover. Name tests after behavior, for example `parse_file_collects_module_name`. Every `parts/` directory except `parts/failing` and `parts/roms` is golden green corpus, gated by `crates/svsim/tests/corpus_gate.rs`; `parts/failing` is the negative corpus, gated as must-fail by the same file (update its diagnostic-fragment tables when you change suites or messages there).

## Commit & Pull Request Guidelines

Follow a simple convention:

- commit subjects in imperative mood, under 72 characters
- one logical change per commit
- include the commands you ran in the PR description

PRs should describe scope, note any unsupported constructs, and include sample CLI output for parser/compiler changes. Add screenshots only when rendering work is involved.

## Architecture Notes

This project is library-first. The CLI should wrap library APIs, not duplicate compiler or simulator logic. The JSON expectation suites under `parts/` are the behavior oracle; the original Python implementation's single-file architecture is explicitly not a model for the Rust codebase.
