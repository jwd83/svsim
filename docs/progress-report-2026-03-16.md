# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after making whole-corpus JSON regressions practical was to separate compile coverage from runtime regression coverage.
- `svsim` now exposes compile-only corpus reporting through `Compiler::run_compile_dir`, `Compiler::run_compile_dirs`, and repeated CLI `--compile-dir` flags.
- Compile-only reports are intentionally stricter than raw parse success: a file is marked failing if compilation errors occur or if any lowered module carries unsupported-feature diagnostics.
- The checked-in corpus now has two explicit health signals: `125/125` SystemVerilog source files compile cleanly in about `1.7s`, and `127/127` JSON suites pass in about `18.4s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added compile-only batch report types in `svsim` for per-file, per-directory, and multi-directory corpus runs.
- Added `Compiler::run_compile_dir` and `Compiler::run_compile_dirs` to discover `*.sv` files recursively, compile them in parallel within each directory, and emit deterministic structured reports.
- Added repeated CLI `--compile-dir` support so the compile-only path matches the existing JSON batch runner shape.
- Compile-only reports now surface unsupported lowering diagnostics directly instead of counting every parseable file as success.
- Added unit and CLI coverage for successful compile-only runs, unsupported-feature failures, hard compile failures, and multi-directory aggregation.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.7s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.8s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `41/41` in about `0.7s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `18.4s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.8s`
  - `parts/testing`: `40/40` in about `0.1s`
  - `parts/overture`: `43/43` in about `10.5s`

## Recommended Follow-Up

- Tighten unsupported-construct diagnostics where wider syntax coverage finds partial-lowering gaps.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
target/debug/svsim --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
target/debug/svsim --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
