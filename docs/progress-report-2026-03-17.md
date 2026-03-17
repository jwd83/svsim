# Rewrite Progress Report

Date: March 17, 2026

## Executive Summary

- The last four commits materially changed the repository state: `parts/rv32i` is now a real checked-in demo corpus, and the Rust simulator is exercising shift, compare, branch, jump, subword memory, fence, `ecall`, `ebreak`, and illegal-instruction flows through a compact RV32I core.
- The next defensible implementation step after that trap-and-control expansion was closing the remaining obvious aliasing hole in the demo core: misaligned halfword and word data accesses no longer silently reuse truncated word indices.
- `rv32i_cpu.sv` now raises load-address and store-address traps with causes `4` and `6` for misaligned `LH`/`LHU`/`LW` and `SH`/`SW` operations, and the corpus adds dedicated regression suites for both cases.
- The checked-in green corpus is larger than the older docs claimed: `128/128` SystemVerilog source files compile cleanly in about `2.4s`, and `140/140` JSON suites pass in about `19.9s`. The negative compile-only corpus remains `2/6` as expected.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still the current direct HIR interpreter rather than the future compiled-IR architecture from the long-term plan.

## What Changed Today

- Added misaligned load/store trap handling to `parts/rv32i/rv32i_cpu.sv` for halfword and word data accesses.
- Added `parts/rv32i/demo_misaligned_load.txt` + `parts/rv32i/demo_misaligned_load.json` to pin load-address trap behavior.
- Added `parts/rv32i/demo_misaligned_store.txt` + `parts/rv32i/demo_misaligned_store.json` to pin store-address trap behavior and verify memory remains unchanged on the faulting step.
- Refreshed the RV32I README, the failing-corpus README, and the long-form port plan so the checked-in docs match the current green corpus and trap surface.

## Verified Current State

- `cargo test`: pass
- focused RV32I regression: `cargo test run_json_test_dir_passes_rv32i_corpus -- --nocapture`: pass
- compile-only multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i`): `128/128` in about `2.4s`
- compile-only per-directory status:
  - `parts/basic`: `44/44` in about `0.9s`
  - `parts/testing`: `42/42` in about `0.2s`
  - `parts/overture`: `41/41` in about `0.8s`
  - `parts/rv32i`: `1/1` in about `0.4s`
- JSON regression multi-directory corpus (`parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i`): `140/140` in about `19.9s`
- JSON regression per-directory status from the same fresh run:
  - `parts/basic`: `44/44` in about `8.0s`
  - `parts/testing`: `42/42` in about `0.2s`
  - `parts/overture`: `43/43` in about `10.4s`
  - `parts/rv32i`: `11/11` in about `1.3s`
- compile-only negative corpus (`parts/failing`): `2/6` in about `0.0s`

## Recommended Follow-Up

- Keep the RV32I demo core honest with targeted corpus additions instead of broad undocumented behavior changes; instruction-address misalignment is the next obvious trap slice if the demo CPU keeps growing.
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
