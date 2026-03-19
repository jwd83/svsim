# Rewrite Progress Report

Superseded by `docs/progress-report-2026-03-18.md`.

Date: March 17, 2026

## Executive Summary

- The recent RV32I commit series materially changed the repository state: `parts/rv32i` is now a real checked-in demo corpus, and the Rust simulator is exercising shift, compare, branch, jump, subword memory, fence, `ecall`, `ebreak`, and illegal-instruction flows through a compact RV32I core.
- The next defensible implementation step after the recent trap-and-control expansion was closing the remaining obvious address-aliasing hole in the demo core: aligned fetch, load, and store addresses outside the 64-word demo memories no longer silently reuse truncated word indices.
- `rv32i_cpu.sv` now raises instruction, load, and store access-fault traps with causes `1`, `5`, and `7` for aligned out-of-range accesses, in addition to the existing instruction/load/store misalignment traps with causes `0`, `4`, and `6`.
- Shared helper functions that were duplicated between `sim.rs` and `validate.rs` have been extracted into `width.rs` and `hir.rs`, reducing cross-module duplication by ~70 lines while keeping all 92 tests green.
- The checked-in green corpus remains `128/128` compile-clean SystemVerilog source files, and the JSON regression surface remains `145/145` passing suites. The negative compile-only corpus remains `2/6` as expected.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added explicit aligned out-of-range access-fault handling to `parts/rv32i/rv32i_cpu.sv` so `imem` and `dmem` no longer silently alias high address bits.
- Added `parts/rv32i/demo_fetch_access_fault.txt` + `parts/rv32i/demo_fetch_access_fault.json` to pin instruction fetch access faults with cause `1`.
- Added `parts/rv32i/demo_load_access_fault.txt` + `parts/rv32i/demo_load_access_fault.json` to pin aligned out-of-range load access faults with cause `5`.
- Added `parts/rv32i/demo_store_access_fault.txt` + `parts/rv32i/demo_store_access_fault.json` to pin aligned out-of-range store access faults with cause `7` and verify memory is unchanged.
- Refreshed the RV32I README, the checked-in RV32I JSON report, and the long-form port plan so the docs match the current trap surface and suite count.
- Extracted five duplicated helper functions into shared locations:
  - `mask`, `shift_bits`, `shift_left_bits`, `shift_right_bits`, `ShiftDirection` moved from both `sim.rs` and `validate.rs` into `width.rs`
  - `expr_to_lvalue` moved from both `sim.rs` and `validate.rs` into `hir.rs`
  - `resolve_legacy_rom_data_path` made `pub(crate)` in `validate.rs` so `sim.rs` imports it instead of duplicating it

## Verified Current State

- `cargo test`: pass (92 tests: 84 unit + 8 integration)
- `cargo test run_compile_dir`: pass
- `cargo test run_json_test_dir`: pass (includes `145/145` JSON regression suites and `128/128` compile-only surface)
- compile-only negative corpus (`parts/failing`): `2/6` as expected

## Recommended Follow-Up

- Keep the RV32I demo core honest with targeted corpus additions instead of broad undocumented behavior changes; the obvious address-aliasing hole is now closed, so future RV32I growth should stay equally surgical.
- Continue consolidating validation: now that the shared helpers are centralized, the next step is to identify runtime checks in `sim.rs` that duplicate compile-time validation and trust the validation pass instead of re-checking at runtime.
- If wider-than-64 designs become a real target, introduce a wider `Value` representation instead of only raising the validator ceiling.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo test run_compile_dir -- --nocapture
cargo test run_json_test_dir -- --nocapture
```
