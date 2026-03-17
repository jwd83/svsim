# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after the duplicate-module registration fix was moving the current 64-bit runtime width boundary forward into compile-time validation.
- `svsim` now rejects ports, signals, memory elements, literals, concatenation/replication expressions, and concatenated assignment targets that exceed the current 64-bit runtime representation, so those failures no longer wait until simulation.
- Compilation now rejects duplicate module definitions, duplicate declarations, duplicate instance names, undeclared identifiers and memories, out-of-range lowered selects, unsupported port directions, bad instance port names, duplicate named port connections, missing child input bindings, non-lvalue output-port bindings, input-port assignment targets, and overwide value-shape constructs before simulation starts.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `2.4s`, and `127/127` JSON suites pass in about `19.1s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added width-limit validation for module declarations so ports, signals, and memory elements wider than `64` bits fail during compilation.
- Added expression and assignment-target validation for literals, concatenations, replications, and concatenated lvalues that exceed the current runtime representation.
- Added compiler regression tests covering overwide ports, concatenation expressions, and concatenated assignment targets.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `2.4s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `1.1s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `41/41` in about `1.1s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `19.1s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `8.8s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.1s`

## Recommended Follow-Up

- Continue elaboration work on width normalization within the supported `1..=64` bit subset so truncation and extension behavior stop being split between compilation and `sim`.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
