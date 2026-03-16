# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after separating compile-only coverage from JSON regression coverage was to stop treating compilation as mostly structural.
- `svsim` now runs a first semantic validation pass over lowered HIR during compilation, so obvious bad designs fail at `compile_file` / `compile_str` time instead of surviving until simulation.
- The new validator currently checks duplicate declarations, undeclared identifiers and memories, out-of-range lowered selects, bad instance port names, duplicate named port connections, and output-port bindings that are not lvalues.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.9s`, and `127/127` JSON suites pass in about `16.7s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added an internal `validate` pass that walks lowered HIR after module discovery and rejects semantically invalid designs before a `CompiledDesign` is returned.
- The validation pass covers duplicate declarations, expression/lvalue identifier resolution, memory references, bit/part-select bounds, `always_ff` clock lookup, and instance port binding shape checks.
- Invalid named-port instance bindings now fail during compilation instead of only when a simulation path happens to touch the affected instance.
- Added compiler regression tests for undeclared identifiers, duplicate declarations, unknown instance ports, duplicate port connections, and non-lvalue output bindings.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.9s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.9s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `41/41` in about `0.8s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `16.7s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.7s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `8.8s`

## Recommended Follow-Up

- Extend compile-time validation into a more explicit elaboration pass so width checks, port binding rules, and hierarchy constraints stop being split between `validate` and `sim`.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
target/debug/svsim --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
target/debug/svsim --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
