# Rewrite Progress Report

Date: March 19, 2026

## Executive Summary

- The next defensible PicoRV32 step after the new load-backed regression was still not another blind semantics patch. A minimal traced compare program showed that the current rewrite already handles signed-vs-unsigned compare execution correctly too; that behavior just was not yet represented in the checked-in executable corpus.
- I added that compare-heavy regression, so the green PicoRV32 runtime surface now covers not only straight-line execution, multi-store continuation, taken branches, jump/link control flow, and a load-backed datapath case, but also a real signed/unsigned compare-and-branch distinction through PicoRV32 itself.

## What Changed Today

- Confirmed with hierarchical traces that PicoRV32 already handles signed and unsigned ordering distinctly under the current simulator: the traced sample computes `slt(x2, x3) = 1`, `sltu(x2, x3) = 0`, takes `blt`, does not take `bltu`, stores those visible results into RAM, and traps cleanly at PC `48`.
- Added `parts/picorv32/demo_compare_branch.txt` and `parts/picorv32/demo_compare_branch.json`, proving that checked-in PicoRV32 execution now covers compare-heavy control flow rather than only branch, jump, and load-backed datapath cases.
- Added a Rust library regression that runs the new PicoRV32 compare-branch JSON suite through `Compiler::run_json_file`.
- Regenerated the checked-in PicoRV32 JSON directory report to include the new ninth suite.

## Verified Current State

- `cargo test`: pass (`122/122`)
- `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`9/9`)
- `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`162/162`)

## Recommended Follow-Up

- Use the same hierarchical-trace workflow to push PicoRV32 into subword memory execution next. Signed/unsigned compare control is now green too; the next real runtime work is byte-lane stores plus sign/zero-extending loads that stress the native RAM interface beyond full-word traffic.
- Keep the docs honest about the boundary between compile-only and executable coverage. `picorv32.v` is now in the measured green compile corpus, but the checked-in executable corpus is still intentionally narrower.

## Commands Run

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```
