# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- Recent commits have been pushing obvious static failures out of `sim` and into compilation: duplicate module definitions, duplicate instance names, missing child input bindings, bad port directions, malformed legacy `rom_*` wrappers, and runtime-width boundary checks now fail before simulation starts.
- After reviewing that trajectory and the current codebase, the next defensible step was finishing the obvious in-range width-coercion slice inside the supported `1..=64` bit subset. `svsim` now makes assignment and instance-port coercion explicit instead of relying on scattered incidental truncation and zero-extension through raw `u64` handoff.
- The current `64`-bit ceiling is an implementation boundary, not a SystemVerilog claim. The runtime is still built around an inline `u64` value representation, so supporting wider designs requires a wider `Value` representation rather than only changing a validator constant.
- The checked-in corpus stayed green after the change, and the all-green surface is now slightly larger: `126/126` SystemVerilog source files compile cleanly in about `1.2s`, and `128/128` JSON suites pass in about `18.3s`.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added an explicit runtime coercion path for assignments and instance-port handoff so width changes happen through one `Value`-level operation instead of a mix of ad hoc `Value::new` calls and raw `u64` maps.
- Instance-input cache keys now use the coerced child-visible port value, which avoids needless child re-evaluation when only truncated-away parent bits change.
- Added focused regression coverage in `sim.rs` plus a new green-corpus fixture, `parts/testing/020-WidthCoercion.sv` and `parts/testing/020-WidthCoercion.json`, to exercise widened and narrowed assignment and port connections.
- Updated the long-form port plan to reflect that the self-determined ternary, assignment, and instance-port coercion slices are now landed inside the current `u64` runtime boundary.

## Verified Current State

- `cargo test`: pass
- direct JSON regression for `parts/testing/020-WidthCoercion`: `3/3`
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `126/126` in about `1.2s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.5s`
  - `parts/testing`: `41/41` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.5s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture`): `128/128` in about `18.3s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.4s`
  - `parts/testing`: `41/41` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.7s`

## Recommended Follow-Up

- Keep routing width-shape questions through shared helpers instead of reintroducing one-off rules in validation, assignment, and port-evaluation code.
- Continue moving in-range resolution and connection-shape checks into a fuller elaboration/validation layer now that the first runtime coercion slice is explicit.
- If wider-than-64 designs become a real target, introduce a wider `Value` representation instead of only raising the validator ceiling.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo fmt
cargo test
cargo run -q -p svsim-cli -- parts/testing/020-WidthCoercion.sv --json-test parts/testing/020-WidthCoercion.json
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture
```
