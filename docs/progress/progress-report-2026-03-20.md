# Rewrite Progress Report

Date: March 20, 2026

## Executive Summary

- The documented next PicoRV32 runtime gap was subword memory, and the current rewrite was already close enough to justify proving that path with a checked-in executable regression instead of widening simulator semantics again.
- I added a PicoRV32 subword-memory suite that drives `SB`, `SH`, `LB`, `LBU`, `LH`, and `LHU` through the native RAM window and stores visible results before trapping.

## What Changed Today

- Added `parts/picorv32/demo_subword_mem.txt` and `parts/picorv32/demo_subword_mem.json`.
- The new program stores `0x80ff3344`, reads the high byte and high halfword in both signed and unsigned forms, mutates the same word with `sb` and `sh`, and stores visible derived results before executing `ebreak`.
- The checked-in end state proves:
  - byte-lane writes update RAM word `0` to `0x06783344`
  - `lb` sign-extends to `0xffffff80`
  - `lbu` zero-extends to `0x00000080`
  - `lh` and `lhu` diverge only in their upper 16 bits, producing a stored delta of `0x00010000`
- Added the matching Rust library regression in `crates/svsim/src/test.rs`.
- Corrected the PicoRV32 harness docs to match reality: the executable RAM window is currently the first four words at `0x0000_0100` through `0x0000_010f`, not a wider 16-word region.
- Regenerated the checked-in PicoRV32 JSON directory report to include the new tenth suite.

## Verified Current State

- `cargo test`: pass (`125/125`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`10/10`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`163/163`)

## Recommended Follow-Up

- Push PicoRV32 into explicit misaligned-access and trap behavior next. Full-word and subword memory traffic are now represented in the executable corpus; the next defensible boundary is whether PicoRV32 traps correctly on misaligned native-bus accesses under the current rewrite.
- Keep compile-only and executable coverage clearly separated in the docs. `picorv32.v` is compile-green across the full frontend and HIR pipeline, but runtime coverage is still a curated subset.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
