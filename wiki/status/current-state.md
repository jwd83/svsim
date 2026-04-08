# Current State

Snapshot date: 2026-04-08

## Verified Today

- `cargo test`: pass. The workspace currently runs `131` `svsim` tests and `8` CLI tests without failures.
- Primary compile-green corpus: `136/136` across `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i`.
- `parts/picorv32` compile surface: `3/3`.
- Combined executable corpus: `166/166` across `parts/basic`, `parts/testing`, `parts/overture`, `parts/rv32i`, and `parts/picorv32`.

## What That Means

- The repository is in a strong state for the currently supported subset.
- The official all-green compatibility target is still the four-directory set from [../../AGENTS.md](../../AGENTS.md): `basic`, `testing`, `overture`, and `rv32i`.
- `picorv32` is no longer just a parser stress case. The upstream core compiles, and the checked-in harness exercises a curated runtime subset.
- Rendering is still intentionally deferred; the project is centered on structured compile and simulation results.

## Commands Used

```text
cargo test
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i
cargo run -q -p svsim-cli -- --compile-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32
```

## Relationship To Older Docs

- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md) still accurately describes the simulator architecture and intended subset.
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md) remains the latest checked-in progress note, but the exact Rust test count is now higher than that March snapshot.

## Sources

- [../../AGENTS.md](../../AGENTS.md)
- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md)
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md)
