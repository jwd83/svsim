# Rewrite Progress Report

Date: March 19, 2026

## Executive Summary

- The recent PicoRV32 work already made `picorv32.v` compile cleanly and added hierarchical runtime tracing. The next defensible implementation step turned out to be a frontend correction: restore constant-condition short-circuit pruning without undoing the runtime comparison/logical precedence fix.
- Lowered constant evaluation now short-circuits `&&` and `||`, so dead PicoRV32 branches gated by parameters like `ENABLE_IRQ` stay pruned even when their right-hand side references runtime state.
- A parser regression now pins both sides of that boundary: `mem_wordsize == 0 && reg_op1[1:0] != 0` still lowers to a top-level logical-and with intact comparisons, and constant-false `ENABLE_IRQ && ...` branches prune away before unsupported lowering.
- Compile-clean PicoRV32 and executable PicoRV32 are still different milestones. The checked-in green executable subset now includes the two-store continuation case in addition to the existing straight-line single-store sample programs and the smoke harness.

## What Changed Today

- Kept the earlier hierarchical tracing surface in place: `SimulationSession::read_signal` and dotted JSON trace names are still the main debugging surface for PicoRV32 control-path work.
- Removed the eager logical short-circuit fold from expression lowering, which was incorrectly erasing the right comparison in source like `mem_wordsize == 0 && reg_op1[1:0] != 0` because of `sv-parser`'s parse shape.
- Reintroduced short-circuiting where it actually belongs: `const_eval_param_expr` now evaluates logical `&&` and `||` left-to-right and stops once the result is known.
- Added parser regressions covering both the preserved runtime lowering shape and constant-false procedural-branch pruning.
- Promoted `parts/picorv32/demo_two_store.txt` into the green JSON corpus with a checked-in `demo_two_store.json` regression that proves PicoRV32 executes two visible stores before trapping.

## Verified Current State

- `cargo test`: pass (`117/117`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`5/5`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i --compile-dir parts/picorv32`: pass (`139/139`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`158/158`)

## Recommended Follow-Up

- Use hierarchical traces plus the checked-in PicoRV32 sample corpus to debug control-path execution. The next real runtime work is now taken branches, not post-store continuation and not more frontend surface for this design.
- Keep the docs honest about the boundary between compile-only and executable coverage. `picorv32.v` is now in the measured green compile corpus, but the checked-in executable corpus is still intentionally narrower.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
