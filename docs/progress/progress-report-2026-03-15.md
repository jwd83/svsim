# Rewrite Progress Report

Superseded by `docs/progress-report-2026-03-16.md`.

Date: March 15, 2026

## Executive Summary

- The rewrite remains in solid shape at the simulator-core level: parsing, lowering, hierarchical compilation, combinational evaluation, sequential stepping, memory preload/read APIs, and JSON regression execution are all implemented in `svsim`.
- The most defensible next implementation step from the March 14 report was not another language feature. It was closing a measurement blind spot in corpus discovery: the batch runner skipped JSON suites that intentionally reused an existing `.sv` source file.
- That gap is now closed. Directory regression discovery accepts an explicit JSON `source` field, and the two Overture CPU program suites that previously sat outside normal directory accounting now participate in the same runner path.
- Rendering is still deferred. `svsim-render` remains a placeholder crate, and the runtime is still a direct HIR interpreter rather than the future compiled-IR architecture described in the long-term plan.

## What Changed Today

- `Compiler::run_json_test_dir` now discovers JSON-only suites when the JSON object declares `"source": "<path>.sv"`.
- CLI coverage was extended with an end-to-end regression test for that explicit-source directory mode.
- `parts/overture/overture_cpu_program_branch.json` and `parts/overture/overture_cpu_program_io.json` now declare `source: "overture_cpu.sv"`, so they are counted by normal directory discovery instead of requiring ad hoc manual invocation.

## Verified Current State

- `cargo test`: pass
- `parts/testing` directory regression: `40/40`
- `overture_cpu_program_branch.json` against `overture_cpu.sv`: `1/1`
- `overture_cpu_program_io.json` against `overture_cpu.sv`: `2/2`
- `parts/overture` now contains `43` discoverable JSON suites under the updated rules: `41` sibling-source suites plus `2` explicit-source program variants

Status note:

- I did not record a fresh completed `parts/basic` or whole-`parts/overture` directory timing snapshot in this report because those full directory runs remain noticeably CPU-heavy in this environment. The change made today is about discovery correctness, and the added Overture suites were verified directly through the same compile-and-run path they use under directory execution.

## Recommended Follow-Up

- Record one clean full-directory batch snapshot for `parts/basic`, `parts/testing`, and `parts/overture` now that shared-source JSON suites are no longer excluded from discovery.
- Add compile-only corpus coverage if future progress reporting needs to include `.sv` files that intentionally do not ship with sibling JSON suites.
- Keep rendering and trace artifacts deferred until the measurement path is fully trustworthy; the current bottleneck is observability, not simulator semantics.

## Commands Run

```text
cargo test
target/debug/svsim --json-test-dir parts/testing
target/debug/svsim parts/overture/overture_cpu.sv --json-test parts/overture/overture_cpu_program_branch.json
target/debug/svsim parts/overture/overture_cpu.sv --json-test parts/overture/overture_cpu_program_io.json
```
