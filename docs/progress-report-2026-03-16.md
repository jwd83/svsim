# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after tightening port-direction semantics and required child input bindings was making instance identity unambiguous for hierarchy inspection and per-instance memory APIs.
- `svsim` now rejects modules that declare the same child instance name more than once during compilation, so compiled designs cannot produce ambiguous hierarchy paths such as `top.u_child`.
- The validator now checks duplicate declarations, duplicate instance names, undeclared identifiers and memories, out-of-range lowered selects, unsupported port directions, bad instance port names, duplicate named port connections, missing child input bindings, non-lvalue output-port bindings, and input-port assignment targets.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `2.3s`, and `127/127` JSON suites pass in about `18.3s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Extended the internal `validate` pass so each module must use unique child instance names.
- Ambiguous instance-path shapes that would break `CompiledDesign::hierarchy()` consumers and per-instance memory APIs now fail during compilation instead of surviving until lookup time.
- Added a compiler regression test covering duplicate instance names alongside the existing named-port validation coverage.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `2.3s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `1.1s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `41/41` in about `1.0s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `18.3s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `8.2s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.0s`

## Recommended Follow-Up

- Extend compile-time validation into a more explicit elaboration pass so width normalization and the remaining hierarchy constraints stop being split between `validate` and `sim`.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
