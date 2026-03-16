# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The rewrite remains functionally solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all implemented in `svsim`.
- The next defensible step from the March 15 report was not more syntax. It was making the existing whole-corpus measurement path practical enough to run regularly on the checked-in designs.
- That bottleneck is now materially reduced. The fixed-point evaluator caches child instance outputs within a `settle_module` pass whenever a child's input map has not changed, which avoids re-running unchanged subtrees across convergence iterations.
- The result is large enough to change project status, not just a benchmark number: the full `parts/basic` + `parts/testing` + `parts/overture` corpus now completes at `127/127` in about `17.2s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still a direct HIR interpreter rather than the future compiled-IR architecture described in the long-term plan.

## What Changed Today

- `crates/svsim/src/sim.rs` now memoizes per-instance child outputs inside each `settle_module` call, keyed by the child input map built for that iteration.
- The optimization is local to the current evaluator. It does not change the supported SystemVerilog subset, public library surface, or JSON report format.
- `docs/rust-port-plan.md` has been updated to reflect that regular whole-corpus measurement is now practical on the checked-in suites.

## Verified Current State

- `cargo test`: pass
- `parts/basic` directory regression: `44/44` in about `7.3s`
- `parts/testing` is still green under directory regression and contributes `40/40` to the full corpus run
- `parts/overture` directory regression: `43/43` in about `10.0s`
- full multi-directory corpus regression (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `17.2s`

Targeted before/after spot checks from this review:

- `parts/basic/adder_cs_64bit.sv` with `adder_cs_64bit.json`: about `85.7s` before, about `2.2s` after
- `parts/overture/overture_cpu.sv` with `overture_cpu_program_io.json`: about `24.7s` before, about `3.0s` after

## Recommended Follow-Up

- Add compile-only corpus coverage if future progress reporting needs to include `.sv` files that intentionally do not ship with sibling JSON suites.
- Tighten unsupported-construct diagnostics if wider corpus additions uncover frontend or runtime gaps.
- Keep rendering and trace artifacts deferred until there is a concrete consumer for them; the core simulator and measurement path are now in better shape than the output layer.

## Commands Run

```text
cargo test
target/debug/svsim parts/basic/adder_cs_64bit.sv --json-test parts/basic/adder_cs_64bit.json
target/debug/svsim parts/overture/overture_cpu.sv --json-test parts/overture/overture_cpu_program_io.json
target/debug/svsim --json-test-dir parts/basic
target/debug/svsim --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
