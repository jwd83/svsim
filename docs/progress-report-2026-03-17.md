# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- Recent commits have been pushing obvious static failures out of `sim` and into compilation: duplicate module definitions, duplicate instance names, missing child input bindings, bad port directions, malformed legacy `rom_*` wrappers, and runtime-width boundary checks now fail before simulation starts.
- After the width-coercion slice landed, the next defensible step was moving the remaining obvious constant memory-bounds failures into compilation. `svsim` now rejects constant out-of-range memory reads and writes during validation instead of surfacing them later as runtime `memory index [...] is out of range` errors.
- The current `64`-bit ceiling is an implementation boundary, not a SystemVerilog claim. The runtime is still built around an inline `u64` value representation, so supporting wider designs requires a wider `Value` representation rather than only changing a validator constant.
- The checked-in green corpus stayed green after the change: `126/126` SystemVerilog source files compile cleanly in about `1.6s`, and `128/128` JSON suites pass in about `18.6s`. The negative corpus now includes an explicit constant-memory-index failure fixture, and compile-only status there is `2/6` as expected.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added a small constant-expression evaluator inside validation so constant memory indices can be resolved against declared unpacked array bounds before simulation starts.
- Compile-time validation now rejects constant out-of-range memory reads and writes instead of deferring those failures to `sim`.
- Added compiler regression tests for out-of-range constant memory reads and writes, updated the CLI failing-corpus regression to account for the new case, and added `parts/failing/constant_memory_index_oob.sv` plus `parts/failing/constant_memory_index_oob.json`.
- Updated the long-form port plan to record that constant memory-bounds checking is now part of the compile-time validation surface.

## Verified Current State

- `cargo fmt`: pass
- `cargo test`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `126/126` in about `1.6s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.8s`
  - `parts/testing`: `41/41` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.7s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `128/128` in about `18.6s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `8.1s`
  - `parts/testing`: `41/41` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.3s`
- compile-only negative corpus (`parts/failing`): `2/6`, with `constant_memory_index_oob.sv` now failing during compilation on the expected out-of-range memory index diagnostic

## Recommended Follow-Up

- Keep routing static-shape questions through shared helpers instead of reintroducing one-off rules in validation and `sim`.
- Continue moving in-range resolution and connection-shape checks into a fuller elaboration/validation layer now that width coercion and constant memory-bounds checks are both explicit.
- If wider-than-64 designs become a real target, introduce a wider `Value` representation instead of only raising the validator ceiling.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo fmt
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
cargo run -q -p svsim-cli -- --compile-dir parts/failing
```
