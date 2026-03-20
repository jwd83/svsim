# Rewrite Progress Report

Date: March 19, 2026

## Executive Summary

- The next defensible PicoRV32 step was not a larger control-path feature but a narrower semantic correction: unsized decimal integer literals were too narrow, which broke full-width masks like `& ~1` and sent taken branch targets to `0x0000_0000`.
- Lowering now treats unsized decimal integers as 32-bit values, which matches the integer semantics PicoRV32 relies on for masked control-flow targets.
- That fix was promoted immediately into the checked-in corpus with a PicoRV32 taken-`beq` regression, so the green runtime surface now covers a real taken conditional branch in addition to the earlier straight-line, multi-store, and smoke cases.

## What Changed Today

- Narrowed the root cause of the PicoRV32 branch failure with hierarchical trace reads: `latched_branch` asserted, but `reg_pc` snapped to zero because `~1` was being computed with width 1 instead of width 32.
- Updated decimal-number lowering so unsized literals like `1` become `32`-bit HIR literals, which makes `~1` behave like `32'hffff_fffe` instead of `1'b0`.
- Added a focused simulator regression for that semantic boundary: `assign out = in & ~1` now preserves the upper 31 bits as expected.
- Added `parts/picorv32/demo_branch_taken.txt` and `parts/picorv32/demo_branch_taken.json`, proving a taken `beq` skips the untaken `addi`, stores `42`, and traps cleanly.
- Regenerated the checked-in PicoRV32 JSON directory report to include the new sixth suite.

## Verified Current State

- `cargo test`: pass (`119/119`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`6/6`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`159/159`)

## Recommended Follow-Up

- Use hierarchical traces plus the checked-in PicoRV32 sample corpus to debug jump/link control flow next. Plain taken conditional branches are now green; the next real runtime work is `jal` / `jalr`-style control paths and link-register writeback.
- Keep the docs honest about the boundary between compile-only and executable coverage. `picorv32.v` is now in the measured green compile corpus, but the checked-in executable corpus is still intentionally narrower.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
