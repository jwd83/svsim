# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- The recent RV32I commit series materially changed the repository state: `parts/rv32i` is now a real checked-in demo corpus, and the Rust simulator is exercising shift, compare, branch, jump, subword memory, fence, `ecall`, `ebreak`, and illegal-instruction flows through a compact RV32I core.
- The next defensible implementation step after the recent trap-and-control expansion was closing the remaining obvious address-aliasing hole in the demo core: aligned fetch, load, and store addresses outside the 64-word demo memories no longer silently reuse truncated word indices.
- `rv32i_cpu.sv` now raises instruction, load, and store access-fault traps with causes `1`, `5`, and `7` for aligned out-of-range accesses, in addition to the existing instruction/load/store misalignment traps with causes `0`, `4`, and `6`.
- The checked-in green corpus remains `128/128` compile-clean SystemVerilog source files, and the JSON regression surface grows to `145/145` passing suites in about `19.1s`. The negative compile-only corpus remains `2/6` as expected.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added explicit aligned out-of-range access-fault handling to `parts/rv32i/rv32i_cpu.sv` so `imem` and `dmem` no longer silently alias high address bits.
- Added `parts/rv32i/demo_fetch_access_fault.txt` + `parts/rv32i/demo_fetch_access_fault.json` to pin instruction fetch access faults with cause `1`.
- Added `parts/rv32i/demo_load_access_fault.txt` + `parts/rv32i/demo_load_access_fault.json` to pin aligned out-of-range load access faults with cause `5`.
- Added `parts/rv32i/demo_store_access_fault.txt` + `parts/rv32i/demo_store_access_fault.json` to pin aligned out-of-range store access faults with cause `7` and verify memory is unchanged.
- Refreshed the RV32I README, the checked-in RV32I JSON report, and the long-form port plan so the docs match the current trap surface and suite count.

## Verified Current State

- `cargo test`: pass
- focused RV32I regression: `cargo test run_json_test_dir_passes_rv32i_corpus -- --nocapture`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i`): `128/128` in about `2.1s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.8s`
  - `parts/testing`: `42/42` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.7s`
  - `parts/rv32i`: `1/1` in about `0.4s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i`): `145/145` in about `19.1s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.6s`
  - `parts/testing`: `42/42` in about `0.2s`
  - `parts/overture`: `43/43` in about `9.9s`
  - `parts/rv32i`: `16/16` in about `1.4s`
- compile-only negative corpus (`parts/failing`): `2/6` in about `0.1s`

## Recommended Follow-Up

- Keep the RV32I demo core honest with targeted corpus additions instead of broad undocumented behavior changes; the obvious address-aliasing hole is now closed, so future RV32I growth should stay equally surgical.
- Continue moving static-shape checks into shared elaboration and validation helpers instead of reintroducing one-off rules in `sim`.
- If wider-than-64 designs become a real target, introduce a wider `Value` representation instead of only raising the validator ceiling.
- Continue deferring rendering and trace artifacts until there is a concrete consumer for them; the core compiler/simulator/reporting path is still the highest-value area.

## Commands Run

```text
cargo test
cargo test run_json_test_dir_passes_rv32i_corpus -- --nocapture
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i
cargo run -q -p svsim-cli -- --json-test-dir parts/rv32i
cargo run -q -p svsim-cli -- --compile-dir parts/failing
```
