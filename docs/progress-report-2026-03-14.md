# Rewrite Progress Report

Date: March 14, 2026

## Executive Summary

- The Rust rewrite is well past the bootstrap stage. Parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all implemented in the main `svsim` crate.
- Verified today: `cargo test` passes, `parts/testing` passes `39/40` JSON suites, and `parts/overture` passes `41/41` JSON suites.
- The main known compatibility gap is still `parts/testing/019-Vector5.json`. The Rust engine evaluates multi-expression replication in standard SystemVerilog bit order, while the checked-in JSON preserves a Python reference bug.
- Rendering is still deferred. `svsim-render` is currently a placeholder crate, and the Rust rewrite does not yet expose truth-table, waveform, or trace artifact types.
- The implementation is currently a direct HIR interpreter with string-keyed runtime state, not the separate elaboration/value/compiled-IR architecture described in parts of the plan.

## Verified Current State

Core implementation now present in the tree:

- Frontend parsing and lowering via `sv-parser`, including `compile_file` and `compile_str`, are implemented in `crates/svsim/src/frontend/sv_parser.rs` and `crates/svsim/src/compiler.rs`.
- Recursive module discovery, search-path resolution, virtual-path dependency lookup, and batch JSON suite discovery live in `crates/svsim/src/compiler.rs`.
- The embeddable library surface is real: `Compiler`, `CompiledDesign`, `SimulationSession`, hierarchy inspection, and JSON test entry points are exported from `crates/svsim/src/lib.rs` and `crates/svsim/src/design.rs`.
- JSON combinational and sequential regression execution, including memory-file binding and legacy `pgm_*` auto-binding, are implemented in `crates/svsim/src/test.rs`.
- Hierarchical combinational settling, `always_comb`, `always_ff @(posedge ...)`, nonblocking staging, and runtime memory access APIs are implemented in `crates/svsim/src/sim.rs`.
- `svsim-render` is still intentionally stubbed; its current implementation is only `status() -> "rendering deferred"` in `crates/svsim-render/src/lib.rs`.

Measured status from this review:

- `cargo test`: pass
- `parts/testing`: `39/40`
- `parts/overture`: `41/41`
- `parts/testing/019-Vector5`: `1/7` JSON cases pass, `6/7` fail because expected outputs still reflect the legacy Python replication-order bug

Status caveat:

- `results/` still reflects reference-flow artifacts, not the current Rust rewrite status. In particular, `results/parts_testing.txt` shows `019-Vector5` passing, which does not match the current Rust JSON runner.

## Documentation Mismatches

The main mismatches are in `docs/rust-port-plan.md`.

1. The compatibility target still includes structured truth-table and waveform results as part of the first milestone.
   Current code does not provide `TruthTable`, `WaveTrace`, or trace export APIs, and `svsim-render` is still a stub. The plan itself later defers those features to Phases 4 and 5, so the milestone text is internally inconsistent with the rest of the document and with the implementation.

2. The architecture section still describes internal `svsim::elab` and `svsim::value` modules.
   The actual crate exports `compiler`, `design`, `diag`, `frontend`, `hir`, `sim`, and `test`. There is no separate `elab` module, and the runtime `Value` type is private inside `crates/svsim/src/sim.rs`.

3. The compiled-IR section reads as if the runtime should already compile HIR into a lower-level execution form.
   The current simulator does not do that yet. Runtime state is stored in `HashMap<String, Value>`, and execution walks the lowered HIR directly inside `crates/svsim/src/sim.rs`.

4. The recommended dependency stack still lists `image`, `imageproc`, and `ab_glyph`.
   Those crates are not currently in the Cargo manifests because rendering is still deferred. That section is better read as future dependency intent than current state.

5. Phase 3 says wider measured Overture parity is still pending.
   That is stale after today’s run. The current Rust CLI batch runner passes all `41/41` JSON-backed suites under `parts/overture`.

## Recommended Follow-Up

- Update `docs/rust-port-plan.md` so the "Current Status" and Phase 3 sections include the measured `parts/overture` result.
- Clarify which sections of the port plan describe current implementation versus future architecture. The biggest ambiguity today is the compiled-IR/elaboration/value-layer material.
- Remove truth-table and waveform output from the "first meaningful milestone" wording, or explicitly label it as post-parity work.
- Decide the compatibility policy for `parts/testing/019-Vector5.json`: fix the JSON, preserve the Python bug in a compatibility mode, or document the intentional divergence.
- Run and record a full `parts/basic` batch measurement separately. It was started during this review but did not complete within the review window, so I am not treating it as a verified status number yet.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --json-test-dir parts/testing
cargo run -q -p svsim-cli -- --json-test-dir parts/overture
cargo run -q -p svsim-cli -- parts/basic/full_adder.sv
target/debug/svsim parts/testing/019-Vector5.sv --json-test parts/testing/019-Vector5.json
```
