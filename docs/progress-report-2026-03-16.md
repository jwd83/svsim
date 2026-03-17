# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after tightening port-direction semantics was closing the last obvious named-port gap that the runtime still handled implicitly: omitted child input bindings.
- `svsim` now rejects instances that leave required child input ports unconnected during compilation, so compiled designs no longer silently rely on runtime zero-defaulting for those child inputs.
- The validator now checks duplicate declarations, undeclared identifiers and memories, out-of-range lowered selects, unsupported port directions, bad instance port names, duplicate named port connections, missing child input bindings, non-lvalue output-port bindings, and input-port assignment targets.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.4s`, and `127/127` JSON suites pass in about `19.9s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Extended the internal `validate` pass so named instantiations must bind every child `input` port explicitly.
- Missing child input connections now fail during compilation instead of surviving until simulation and reading back as implicit zeroes.
- Added a compiler regression test covering the missing-input binding case alongside the existing named-port validation coverage.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.4s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.5s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `41/41` in about `0.7s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `19.9s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.7s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `12.1s`

## Recommended Follow-Up

- Extend compile-time validation into a more explicit elaboration pass so width normalization and the remaining hierarchy constraints stop being split between `validate` and `sim`.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
target/debug/svsim --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
target/debug/svsim --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
