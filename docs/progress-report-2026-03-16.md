# Rewrite Progress Report

Date: March 16, 2026

## Executive Summary

- The Rust rewrite remains solid at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all in place.
- The next defensible step after making child instance names unique was closing the same ambiguity at the module-definition level.
- `svsim` now rejects duplicate module definitions even when they appear in the same source file, so module lookup no longer depends on file traversal order when two modules share a name.
- Compilation now rejects duplicate module definitions, duplicate declarations, duplicate instance names, undeclared identifiers and memories, out-of-range lowered selects, unsupported port directions, bad instance port names, duplicate named port connections, missing child input bindings, non-lvalue output-port bindings, and input-port assignment targets.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.9s`, and `127/127` JSON suites pass in about `18.4s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Tightened source registration so a module name can be defined only once across the compiled design, including duplicate definitions inside the same `.sv` file.
- Ambiguous module resolution now fails during compilation instead of silently selecting the first matching definition encountered by `HirDesign::module()`.
- Added compiler regression tests covering duplicate module names in both `compile_file` and `compile_str` flows.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.9s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.9s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `41/41` in about `0.8s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `18.4s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.7s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.5s`

## Recommended Follow-Up

- Extend compile-time validation into a more explicit elaboration pass so width normalization and the remaining runtime-only value-width constraints stop being split between compilation and `sim`.
- Keep using compile-only coverage for new `.sv` additions before they have stable JSON suites.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
