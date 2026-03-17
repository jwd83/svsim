# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after landing the first HIR validator was tightening port-direction semantics that the runtime still handled implicitly.
- `svsim` now rejects unsupported `inout` / `ref` module ports and attempts to drive input ports during compilation, so those designs fail at `compile_file` / `compile_str` time instead of surviving until simulation.
- The validator now checks duplicate declarations, undeclared identifiers and memories, out-of-range lowered selects, unsupported port directions, bad instance port names, duplicate named port connections, non-lvalue output-port bindings, and input-port assignment targets.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.0s`, and `127/127` JSON suites pass in about `17.1s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Extended the internal `validate` pass so the currently supported module port model is explicit: only `input` and `output` ports survive compilation.
- Added compile-time rejections for continuous assignments, procedural assignments, and child output bindings that try to drive an input port.
- Invalid port-direction usage now fails during compilation instead of being silently mis-modeled by the runtime.
- Added compiler regression tests for unsupported `inout` ports, direct input-port drive attempts, and child output bindings wired into parent input ports.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.0s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.4s`
  - `parts/testing`: `40/40` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.5s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `17.1s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `6.8s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.2s`

## Recommended Follow-Up

- Extend compile-time validation into a more explicit elaboration pass so connection completeness, width normalization, and the remaining hierarchy constraints stop being split between `validate` and `sim`.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
