# Current State

Snapshot date: 2026-04-17

## Verified Today

- `cargo test`: pass. The workspace runs `168` `svsim` tests and `10` CLI tests without failures.
- Official executable green corpus: `157/157` across `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i` (includes `024-FourStateLiterals` and `025-TopLevelInout`).
- Auxiliary executable corpora: `13/13` in `parts/picorv32`, `9/9` in `parts/sap2` (program suites plus `sap2_bus_semantics`, `sap2_register_tile`, `sap2_inout_top`), `6/6` in `parts/sap1`, `5/5` in `parts/simple8`, and `4/4` in the new `parts/sap3` sketch (AND/OR/XOR plus memory-mapped output port at `0x10`, 20-bit microcode, no simulator changes required).
- Combined executable coverage from the commands below: `194/194`.

## What That Means

- The repository is in a strong state for the currently supported subset, and the verified baseline is wider than the April 8 bootstrap snapshot.
- The official all-green compatibility target is still the four-directory set from [../../AGENTS.md](../../AGENTS.md): `basic`, `testing`, `overture`, and `rv32i`.
- Four-state runtime values, four-state JSON/CLI boundaries, native four-state literals (`8'bz`/`8'bx`), and focused boundary tests are part of the verified baseline.
- Public/top-level `inout` is now supported. The JSON harness drives a bus input either as a two-state number (contender) or `"8'bz"` (released), and observed inout ports are reported with their resolved four-state value. Internal whole-net `inout` leaf ports continue to work behind output-only tops.
- `picorv32` is no longer just a parser stress case. The upstream core compiles, and the checked-in harness exercises a curated runtime subset.
- `sap2` is no longer just a plan target. It is a runnable auxiliary corpus with both long-running program suites and a focused bus-semantics smoke suite.
- Rendering is still intentionally deferred; the project is centered on structured compile and simulation results.

## Commands Used

```text
cargo test
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/sap1
cargo run -q -p svsim-cli -- --json-test-dir parts/sap2
cargo run -q -p svsim-cli -- --json-test-dir parts/sap3
cargo run -q -p svsim-cli -- --json-test-dir parts/simple8
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
