# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- The recent RV32I commit series materially changed the repository state: `parts/rv32i` is now a real checked-in demo corpus, and the Rust simulator is exercising shift, compare, branch, jump, subword memory, fence, `ecall`, `ebreak`, and illegal-instruction flows through a compact RV32I core.
- The next defensible implementation step after the recent trap-and-control expansion was closing the remaining obvious fetch-side aliasing hole in the demo core: taken branch and jump targets with `pc[1:0] != 0` no longer silently reuse truncated `imem` word indices.
- `rv32i_cpu.sv` now raises instruction-address-misaligned traps with cause `0` for taken misaligned branch and jump targets, in addition to the existing load-address and store-address traps with causes `4` and `6` for misaligned `LH`/`LHU`/`LW` and `SH`/`SW` operations.
- The checked-in green corpus remains `128/128` compile-clean SystemVerilog source files, and the JSON regression surface grows to `142/142` passing suites in about `17.7s`. The negative compile-only corpus remains `2/6` as expected.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added instruction-address misalignment trap handling to `parts/rv32i/rv32i_cpu.sv` for taken branch, `JAL`, and `JALR` targets with `pc[1:0] != 0`.
- Added `parts/rv32i/demo_instr_misaligned_branch.txt` + `parts/rv32i/demo_instr_misaligned_branch.json` to pin misaligned taken-branch trap behavior.
- Added `parts/rv32i/demo_instr_misaligned_jalr.txt` + `parts/rv32i/demo_instr_misaligned_jalr.json` to pin misaligned `JALR` trap behavior and verify `rd` is not written on the faulting step.
- Refreshed the RV32I README, the checked-in RV32I JSON report, and the long-form port plan so the docs match the current trap surface and suite count.

## Verified Current State

- `cargo test`: pass
- focused RV32I regression: `cargo test run_json_test_dir_passes_rv32i_corpus -- --nocapture`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i`): `128/128` in about `1.6s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.4s`
  - `parts/testing`: `42/42` in about `0.1s`
  - `parts/overture`: `41/41` in about `0.6s`
  - `parts/rv32i`: `1/1` in about `0.4s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i`): `142/142` in about `17.7s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `7.1s`
  - `parts/testing`: `42/42` in about `0.2s`
  - `parts/overture`: `43/43` in about `9.1s`
  - `parts/rv32i`: `13/13` in about `1.4s`
- compile-only negative corpus (`parts/failing`): `2/6` in about `0.0s`

## Recommended Follow-Up

- Keep the RV32I demo core honest with targeted corpus additions instead of broad undocumented behavior changes; explicit instruction/data address-range faults are the next obvious aliasing hole now that alignment traps are covered.
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
