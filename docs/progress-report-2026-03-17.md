# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- Recent commits have been pushing obvious static failures out of `sim` and into compilation: duplicate module definitions, duplicate instance names, missing child input bindings, bad port directions, and 64-bit runtime width limits now fail before simulation starts.
- After reviewing that trajectory and the current codebase, the next defensible step was tightening the remaining legacy `rom_*` compatibility path. Those wrappers were still validated only implicitly at instantiation time, and malformed wrappers could even degrade into empty modules.
- `svsim` now validates legacy `rom_*` primitives at compile time. Supported wrappers must be port-only modules with exactly one input address port, exactly one output data port, a non-empty `rom_` suffix, a resolvable backing `*.txt` data file, and an address width that fits the host.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.3s`, and `127/127` JSON suites pass in about `17.2s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added compile-time validation for legacy `rom_*` wrappers so malformed compatibility modules fail during `Compiler::compile_file` / `compile_str` instead of waiting for simulation.
- Added compiler regression tests covering both malformed wrapper structure and missing ROM backing files.
- Updated the long-form port plan to record that legacy ROM wrapper validation now belongs to the compile-time validation pass.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.3s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.5s`
  - `parts/testing`: `40/40` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.7s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `17.2s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `6.9s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.1s`

## Recommended Follow-Up

- Continue elaboration work on width normalization within the supported `1..=64` bit subset so truncation and extension behavior stop being split between compilation and `sim`.
- Keep extending compile-time validation where the runtime still contains static fallback checks, especially when the supported subset depends on narrow compatibility conventions like the legacy corpus helpers.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
