# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- Recent commits have been pushing obvious static failures out of `sim` and into compilation: duplicate module definitions, duplicate instance names, missing child input bindings, bad port directions, malformed legacy `rom_*` wrappers, and runtime-width boundary checks now fail before simulation starts.
- After reviewing that trajectory and the current codebase, the next defensible step was taking the first real width-normalization slice inside the supported `1..=64` bit subset. `svsim` now keeps lowered ternary expressions at a fixed self-determined width during evaluation instead of letting the chosen branch shrink the runtime shape.
- The current `64`-bit ceiling is an implementation boundary, not a SystemVerilog claim. The runtime is still built around an inline `u64` value representation, so supporting wider designs requires a wider `Value` representation rather than only changing a validator constant.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.0s`, and `127/127` JSON suites pass in about `17.1s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added a shared lowered-expression width helper so runtime code can ask for the same static width shape that validation already reasons about.
- Normalized ternary evaluation to the expression's self-determined width before the selected branch flows into concatenation, replication, or later assignment coercion.
- Added runtime regression tests covering ternary width normalization inside concatenation and replication.
- Updated the long-form port plan to reflect that self-determined ternary width normalization is now landed, while assignment and port coercions remain the next width-normalization slice.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.0s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.4s`
  - `parts/testing`: `40/40` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.5s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `17.1s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.2s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `9.7s`

## Recommended Follow-Up

- Continue elaboration work on width normalization within the supported `1..=64` bit subset for assignment and port coercions so truncation and extension behavior stop being split between compilation and `sim`.
- Keep routing width-shape questions through the shared lowered-expression helper instead of duplicating width rules between validation and the runtime.
- If wider-than-64 designs become a real target, introduce a wider `Value` representation instead of only raising the validator ceiling.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
