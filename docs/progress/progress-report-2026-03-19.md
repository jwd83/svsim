# Rewrite Progress Report

Date: March 19, 2026

## Executive Summary

- The next defensible PicoRV32 step after the unsized-literal fix was not another broad semantics patch. A minimal traced program showed that `jal` and masked `jalr` already execute correctly under the current simulator, including both link-register writebacks.
- I promoted that already-working behavior into the checked-in PicoRV32 corpus with a new jump/link regression, so the green runtime surface now covers real taken branch, `jal`, and `jalr` control flow in addition to the earlier straight-line, multi-store, and smoke cases.

## What Changed Today

- Confirmed with hierarchical traces that PicoRV32 now steps through `jal` and masked `jalr` correctly: the traced sample writes link values `8` and `16`, lands on address `24`, stores `42`, and traps cleanly at PC `36`.
- Added `parts/picorv32/demo_jump_link.txt` and `parts/picorv32/demo_jump_link.json`, proving that checked-in PicoRV32 execution now covers both jump target selection and link-register writeback.
- Added a Rust library regression that runs the new PicoRV32 jump/link JSON suite through `Compiler::run_json_file`.
- Regenerated the checked-in PicoRV32 JSON directory report to include the new seventh suite.

## Verified Current State

- `cargo test`: pass (`120/120`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`7/7`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`160/160`)

## Recommended Follow-Up

- Use the same hierarchical-trace workflow to push PicoRV32 beyond pure control flow next. `jal` / `jalr` targets and link-register writeback are now green; the next real runtime work is richer datapath coverage such as load-backed or compare-heavier checked-in programs.
- Keep the docs honest about the boundary between compile-only and executable coverage. `picorv32.v` is now in the measured green compile corpus, but the checked-in executable corpus is still intentionally narrower.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
