# Rewrite Progress Report

Date: March 20, 2026

## Executive Summary

- The next documented PicoRV32 runtime gap after subword memory was explicit misaligned native-bus behavior. A quick probe showed the current rewrite already trapped on misaligned `lw`, but the harness still committed a visible aligned RAM write before a misaligned `sw` trap settled.
- I fixed that at the harness boundary by delaying visible store commits by one cycle, then added checked-in PicoRV32 misaligned-load and misaligned-store regressions.

## What Changed Today

- Updated `parts/picorv32/picorv32_program_harness.sv` so visible RAM writes commit one cycle after the native-bus write request. That keeps the harness RAM window aligned with PicoRV32 trap behavior on store-side faults instead of recording a transient write that is immediately invalidated by trap state.
- Added `parts/picorv32/demo_misaligned_load.txt` and `parts/picorv32/demo_misaligned_load.json`.
- Added `parts/picorv32/demo_misaligned_store.txt` and `parts/picorv32/demo_misaligned_store.json`.
- Added matching Rust library regressions in `crates/svsim/src/test.rs`.
- Regenerated the checked-in PicoRV32 JSON directory report to include the new eleventh and twelfth suites.

## Verified Current State

- `cargo test`: pass (`127/127`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`12/12`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`165/165`)

## Recommended Follow-Up

- Push PicoRV32 into instruction-path trap coverage next. Data-side misaligned `lw` and `sw` behavior is now represented in the checked-in executable corpus; the next bounded boundary is taken misaligned branch or `jalr` targets under the current rewrite.
- Keep compile-only and executable coverage clearly separated in the docs. `picorv32.v` is compile-green across the full frontend and HIR pipeline, but runtime coverage is still a curated subset.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
