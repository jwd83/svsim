# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust rewrite of the SystemVerilog simulator in `ref/`.

- `crates/svsim/`: core library crate. Keep compiler, frontend, HIR, diagnostics, and simulation logic here.
- `crates/svsim-cli/`: thin CLI wrapper over `svsim`.
- `crates/svsim-render/`: deferred rendering crate for truth-table and waveform output.
- `parts/basic/`, `parts/overture/`, `parts/testing/`, `parts/rv32i/`: SystemVerilog corpus and JSON expectations used as the all-green compatibility suite.
- `parts/failing/`: intentionally failing negative corpus for manual compile/test failure checks.
- `ref/`: Python reference implementation and historical planning notes.
- `docs/`: architecture and rewrite plans.

Prefer adding new simulator code under `crates/svsim/src/` before splitting into more crates.

## Build, Test, and Development Commands

- `cargo fmt`: format the Rust workspace.
- `cargo test`: build and run all Rust tests.
- `cargo test -p svsim`: run core library tests only.
- `cargo run -p svsim-cli -- parts/basic/full_adder.sv`: parse a real module and emit JSON.
- `cargo run -p svsim-cli -- --json-test parts/basic/full_adder.json parts/basic/full_adder.sv`: run a JSON regression suite through the Rust CLI and emit a structured report.

When adding support for new language features, verify them against files under `parts/` instead of isolated toy inputs only.
Keep `parts/failing/` out of "green" corpus expectations unless you are explicitly checking failure reporting.

## Coding Style & Naming Conventions

Use 4-space indentation and standard Rust naming:

- `snake_case` for modules, files, and functions
- `CamelCase` for types
- `SCREAMING_SNAKE_CASE` for constants

Keep public APIs small and explicit. `sv-parser` types should stay inside the frontend layer; lower them into owned HIR types before passing data deeper into the system. Prefer explicit configuration for memory/program bindings over hidden naming conventions.

## Testing Guidelines

Use Rust unit tests alongside the code they cover. Name tests after behavior, for example `parse_file_collects_module_name`. Treat `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i` as the golden green corpus, and use `parts/failing` for intentional negative coverage when you need to exercise failure paths.

## Commit & Pull Request Guidelines

There is no usable Git history in this checkout, so follow a simple convention:

- commit subjects in imperative mood, under 72 characters
- one logical change per commit
- include the commands you ran in the PR description

PRs should describe scope, note any unsupported constructs, and include sample CLI output for parser/compiler changes. Add screenshots only when rendering work is involved.

## Architecture Notes

This project is library-first. The CLI should wrap library APIs, not duplicate compiler or simulator logic. Use `ref/` as a behavior oracle, but do not copy its single-file architecture into the Rust codebase.
