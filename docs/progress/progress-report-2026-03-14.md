# Rewrite Progress Report

Superseded by `docs/progress-report-2026-03-15.md`.

Date: March 14, 2026

## Executive Summary

- The Rust rewrite is well past the bootstrap stage. Parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all implemented in the main `svsim` crate.
- Verified today: `cargo test` passes, `parts/testing` passes `39/40` JSON suites, and `parts/overture` passes `41/41` JSON suites.
- Progress measurement is now less ad hoc: the library and CLI can aggregate multiple `--json-test-dir` runs into one structured corpus report instead of requiring separate manual invocations.
- The main known compatibility gap is still `parts/testing/019-Vector5.json`. The Rust engine evaluates multi-expression replication in standard SystemVerilog bit order, while the checked-in JSON preserves a Python reference bug.
- Rendering is still deferred. `svsim-render` is currently a placeholder crate, and the Rust rewrite does not yet expose truth-table, waveform, or trace artifact types.
- The implementation is currently a direct HIR interpreter with string-keyed runtime state, not the separate elaboration/value/compiled-IR architecture described in parts of the plan.

## Verified Current State

Core implementation now present in the tree:

- Frontend parsing and lowering via `sv-parser`, including `compile_file` and `compile_str`, are implemented in `crates/svsim/src/frontend/sv_parser.rs` and `crates/svsim/src/compiler.rs`.
- Recursive module discovery, search-path resolution, virtual-path dependency lookup, and both per-directory and multi-directory JSON suite discovery live in `crates/svsim/src/compiler.rs`.
- The embeddable library surface is real: `Compiler`, `CompiledDesign`, `SimulationSession`, hierarchy inspection, and JSON test entry points are exported from `crates/svsim/src/lib.rs` and `crates/svsim/src/design.rs`.
- JSON combinational and sequential regression execution, including memory-file binding, aggregated corpus report types, and legacy `pgm_*` auto-binding, are implemented in `crates/svsim/src/test.rs`.
- Hierarchical combinational settling, `always_comb`, `always_ff @(posedge ...)`, nonblocking staging, and runtime memory access APIs are implemented in `crates/svsim/src/sim.rs`.
- `svsim-render` is still intentionally stubbed; its current implementation is only `status() -> "rendering deferred"` in `crates/svsim-render/src/lib.rs`.

Measured status from this review:

- `cargo test`: pass
- `parts/testing`: `39/40`
- `parts/overture`: `41/41`
- `parts/testing/019-Vector5`: `1/7` JSON cases pass, `6/7` fail because expected outputs still reflect the legacy Python replication-order bug
- full-corpus measurement path: available through repeated `--json-test-dir` flags, though I am not recording a fresh `parts/basic` number in this update because the long-running batch had not completed yet

Status caveat:

- `results/` still reflects reference-flow artifacts, not the current Rust rewrite status. In particular, `results/parts_testing.txt` shows `019-Vector5` passing, which does not match the current Rust JSON runner.

## Documentation Updates

`docs/rust-port-plan.md` has been aligned with the current tree in the areas that were previously most misleading.

1. The first parity milestone now stops at structured JSON test and corpus reports.
   Truth-table and waveform output remain explicitly deferred work rather than implied current-scope deliverables.

2. The architecture section now lists the implemented module boundaries.
   The plan no longer implies that `svsim::elab` or `svsim::value` already exist as real modules in the repository.

3. The compiled-IR material is now labeled as future architecture.
   The current runtime still interprets lowered HIR directly, and the plan says so.

4. Phase 3 and the current-status section now record the measured Overture result and the new corpus-reporting path.
   The next target is no longer framed as "go measure Overture parity"; that part is already done.

## Recommended Follow-Up

- Decide the compatibility policy for `parts/testing/019-Vector5.json`: fix the JSON, preserve the Python bug in a compatibility mode, or document the intentional divergence.
- Use the new aggregated corpus report to record a full `parts/basic` + `parts/testing` + `parts/overture` measurement once the long-running batch finishes in a clean review window.
- Add compile-only corpus coverage if progress tracking needs to include any `parts/**/*.sv` files that do not have sibling JSON suites.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --json-test-dir parts/testing
cargo run -q -p svsim-cli -- --json-test-dir parts/overture
cargo run -q -p svsim-cli -- parts/basic/full_adder.sv
target/debug/svsim parts/testing/019-Vector5.sv --json-test parts/testing/019-Vector5.json
```
