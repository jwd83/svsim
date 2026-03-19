# Rewrite Progress Report

Date: March 19, 2026

## Executive Summary

- The recent PicoRV32 commit series already made `picorv32.v` compile cleanly through the Rust frontend, HIR lowering, and validation pipeline. The next defensible step was to make the compile-only corpus tooling measure that milestone directly instead of leaving plain Verilog sources outside normal directory reporting.
- `Compiler::run_compile_dir`, `Compiler::run_compile_dirs`, and CLI `--compile-dir` discovery now include both `.sv` and `.v` files, and module-name dependency lookup now searches for both extensions in the current directory and configured search paths.
- That change pulls `parts/picorv32/picorv32.v` into the normal green compile corpus. The measured repository state is now `113/113` Rust tests, `139/139` clean compile-only source files across `parts/basic`, `parts/testing`, `parts/overture`, `parts/rv32i`, and `parts/picorv32`, and `157/157` passing JSON suites across those same directories.
- Compile-clean PicoRV32 and executable PicoRV32 are still different milestones. The checked-in green executable subset remains the existing straight-line single-store sample programs plus the smoke harness; local scratch runs still show taken-branch loops and multi-store programs as the next runtime expansion targets.

## What Changed Today

- Compile-directory source discovery now includes plain Verilog `*.v` files in addition to SystemVerilog `*.sv`.
- Module-name dependency resolution now searches for both `.sv` and `.v` files, so search-path-based compilation can find Verilog children without explicit top-level file selection.
- Added compiler and CLI regressions to pin the new behavior:
  - a library test proving search-path dependency resolution works when the child lives in `child.v`
  - updated compile-directory tests proving `.v` files are counted and reported

## Verified Current State

- `cargo test`: pass (`113/113`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`4/4`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i --compile-dir parts/picorv32`: pass (`139/139`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`157/157`)

## Recommended Follow-Up

- Debug PicoRV32 control-path execution now that compile coverage is explicit and automated. The next real runtime work is taken branches and post-store continuation, not more parser surface for this design.
- Keep the docs honest about the boundary between compile-only and executable coverage. `picorv32.v` is now in the measured green compile corpus, but the checked-in executable corpus is still intentionally narrower.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
