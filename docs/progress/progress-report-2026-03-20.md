# Rewrite Progress Report

Date: March 20, 2026

## Executive Summary

- The next documented PicoRV32 runtime gap after data-side misaligned access coverage was instruction-path trap coverage. A quick probe showed the current rewrite already traps on a misaligned `jalr` target without any new simulator semantics.
- I converted that probe into a checked-in PicoRV32 misaligned-`jalr` regression so the executable corpus now covers one instruction-path trap boundary instead of just data-side misalignment.

## What Changed Today

- Added `parts/picorv32/demo_instr_misaligned_jalr.txt` and `parts/picorv32/demo_instr_misaligned_jalr.json`.
- Added a matching Rust library regression in `crates/svsim/src/test.rs`.
- Regenerated the checked-in PicoRV32 JSON directory report to include the new thirteenth suite.
- Kept the misaligned-`jalr` suite traced through `uut.reg_pc` and `uut.cpu_state` so the checked-in report records the internal control-path transition even though the executable assertion surface stays at the harness boundary.

## Verified Current State

- `cargo test`: pass (`128/128`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`13/13`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`166/166`)

## Recommended Follow-Up

- Push PicoRV32 into the remaining instruction-path trap case next. Misaligned `jalr` is now represented in the checked-in executable corpus; the next bounded boundary is a taken misaligned conditional-branch target under the current rewrite.
- Keep compile-only and executable coverage clearly separated in the docs. `picorv32.v` is compile-green across the full frontend and HIR pipeline, but runtime coverage is still a curated subset.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
