# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- Recent commits have been pushing obvious static failures out of `sim` and into compilation: duplicate module definitions, duplicate instance names, missing child input bindings, bad port directions, malformed legacy `rom_*` wrappers, and runtime-width boundary checks now fail before simulation starts.
- After reviewing that trajectory and the current codebase, the next defensible step was making the lower bound of the current width model explicit. `svsim` now rejects lowered zero-width value shapes at compile time, so the supported runtime subset is explicitly `1..=64` bits rather than just “up to 64”.
- The current `64`-bit ceiling is an implementation boundary, not a SystemVerilog claim. The runtime is still built around an inline `u64` value representation, so supporting wider designs requires a wider `Value` representation rather than only changing a validator constant.
- The checked-in corpus stayed green after the change: `125/125` SystemVerilog source files compile cleanly in about `1.1s`, and `127/127` JSON suites pass in about `17.2s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Tightened compile-time width validation so lowered expressions and assignment targets must stay inside the supported `1..=64` bit subset, rejecting zero-width shapes such as zero-count replications before simulation.
- Added compiler regression tests covering zero-width single-expression and multi-expression replication.
- Updated the long-form port plan to state that the current `64`-bit ceiling comes from the inline `u64` runtime value model and that wider-than-64 support needs a representation change.

## Verified Current State

- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `125/125` in about `1.1s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.4s`
  - `parts/testing`: `40/40` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.5s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `127/127` in about `17.2s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `6.6s`
  - `parts/testing`: `40/40` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.3s`

## Recommended Follow-Up

- Continue elaboration work on width normalization within the supported `1..=64` bit subset for assignment and port coercions so truncation and extension behavior stop being split between compilation and `sim`.
- If wider-than-64 designs become a real target, introduce a wider `Value` representation instead of only raising the validator ceiling.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
