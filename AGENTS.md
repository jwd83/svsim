# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust rewrite of the SystemVerilog simulator in `ref/`.

- `crates/svsim/`: core library crate. Keep compiler, frontend, HIR, diagnostics, and simulation logic here.
- `crates/svsim-cli/`: thin CLI wrapper over `svsim`.
- `crates/svsim-render/`: deferred rendering crate for truth-table and waveform output.
- `parts/basic/`, `parts/overture/`, `parts/testing/`: SystemVerilog corpus and JSON expectations used as the compatibility suite.
- `ref/`: Python reference implementation and historical planning notes.
- `docs/`: architecture and rewrite plans.
- `results/`: generated output artifacts from the reference flow.

Prefer adding new simulator code under `crates/svsim/src/` before splitting into more crates.

## Build, Test, and Development Commands

- `cargo fmt`: format the Rust workspace.
- `cargo test`: build and run all Rust tests.
- `cargo test -p svsim`: run core library tests only.
- `cargo run -p svsim-cli -- parts/basic/full_adder.sv`: parse a real module and emit JSON.

When adding support for new language features, verify them against files under `parts/` instead of isolated toy inputs only.

## Coding Style & Naming Conventions

Use 4-space indentation and standard Rust naming:

- `snake_case` for modules, files, and functions
- `CamelCase` for types
- `SCREAMING_SNAKE_CASE` for constants

Keep public APIs small and explicit. `sv-parser` types should stay inside the frontend layer; lower them into owned HIR types before passing data deeper into the system. Prefer explicit configuration for memory/program bindings over hidden naming conventions.

## Testing Guidelines

Use Rust unit tests alongside the code they cover. Name tests after behavior, for example `parse_file_collects_module_name`. Treat `parts/` as the golden corpus: add or update regression tests whenever you expand supported syntax or fix a simulator bug.

## Commit & Pull Request Guidelines

There is no usable Git history in this checkout, so follow a simple convention:

- commit subjects in imperative mood, under 72 characters
- one logical change per commit
- include the commands you ran in the PR description

PRs should describe scope, note any unsupported constructs, and include sample CLI output for parser/compiler changes. Add screenshots only when rendering work is involved.

## Architecture Notes

This project is library-first. The CLI should wrap library APIs, not duplicate compiler or simulator logic. Use `ref/` as a behavior oracle, but do not copy its single-file architecture into the Rust codebase.
