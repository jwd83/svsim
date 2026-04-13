# Current State

Snapshot date: 2026-04-12

## Verified Today

- `cargo test`: pass. The workspace currently runs `168` `svsim` tests and `10` CLI tests without failures.
- Official executable green corpus: `155/155` across `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i`.
- Auxiliary executable corpora: `13/13` in `parts/picorv32` and `7/7` in `parts/sap2`.
- Combined executable coverage from the commands below: `175/175`.

## What That Means

- The repository is in a strong state for the currently supported subset, and the verified baseline is wider than the April 8 bootstrap snapshot.
- The official all-green compatibility target is still the four-directory set from [../../AGENTS.md](../../AGENTS.md): `basic`, `testing`, `overture`, and `rv32i`.
- Four-state runtime values, four-state JSON/CLI boundaries, and focused boundary tests are now part of the verified baseline.
- Internal whole-net `inout` leaf ports are working behind output-only tops; public/top-level `inout` remains intentionally rejected.
- `picorv32` is no longer just a parser stress case. The upstream core compiles, and the checked-in harness exercises a curated runtime subset.
- `sap2` is no longer just a plan target. It is a runnable auxiliary corpus with both long-running program suites and a focused bus-semantics smoke suite.
- Rendering is still intentionally deferred; the project is centered on structured compile and simulation results.

## Commands Used

```text
cargo test
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/sap2
```

## Relationship To Older Docs

- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md) still accurately describes the simulator architecture and intended subset.
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md) remains useful historical context, but it predates the four-state public boundary flip and the runnable `sap2` corpus.
- [../../plan.md](../../plan.md) is now the clearest raw-source description of the active four-state / `inout` / `sap2` milestone.

## Sources

- [../../AGENTS.md](../../AGENTS.md)
- [../../plan.md](../../plan.md)
- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md)
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md)
- [../../parts/sap2/README.md](../../parts/sap2/README.md)
