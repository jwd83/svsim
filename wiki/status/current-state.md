# Current State

Snapshot date: 2026-07-06

## Verified Today

- `cargo test`: pass, `206/206` — `186` `svsim` unit tests (including `14`
  direct four-state truth-table tests in `crates/svsim/src/logic_ops.rs`),
  `10` corpus gate tests (`crates/svsim/tests/corpus_gate.rs`: nine green
  directories plus `corpus_failing_stays_red` over the negative corpus), and
  `10` CLI integration tests, in roughly fifty seconds of wall time (down
  from roughly two minutes before the second review's step 3).
- Green corpus, now enforced directly by `cargo test`: `195/195` regression
  suites across all nine green `parts/` directories — `44` basic, `55`
  testing, `43` overture, `16` rv32i, `13` picorv32, `6` sap1, `9` sap2, `4`
  sap3, `5` simple8.
- `./test.sh` regenerates the committed `docs/tests/report-parts-*.json`
  reports from a release build and now covers sap2 and sap3 correctly. The
  previous script wrote sap1's results into the sap2 report, so committed
  sap2 "green" data before 2026-07-06 was not sap2 data.

## What That Means

- The official all-green compatibility target is no longer the four-directory
  set from the bootstrap era: every `parts/` directory except `parts/failing`
  (negative corpus) and `parts/roms` (assets) is gated, and a missing or
  emptied corpus directory fails the gate instead of passing silently. Since
  the second review's step 1, `parts/failing` is gated in the opposite
  direction: its eight suites must keep failing with stable diagnostics.
- Since the second review's step 2, instance parameter overrides that would
  change a parameter frozen into lowered HIR (declaration ranges, constant
  selects, unrolled `for` loops, pruned `if`/generate branches) are rejected
  at elaboration with a construct-naming diagnostic
  (`ModuleSummary::frozen_parameters`); runtime-only and default-equal
  overrides are unaffected.
- Since the second review's steps 3–4, the settle loop evaluates directly
  over the indexed runtime frame (`ValueReader` seam,
  `FrameValues`/`OverlayValues` readers, dirty-name commit) and converges on
  tracked changes instead of cloning and deep-comparing the frame each
  iteration; the iteration budget is measured (deepest observed convergence:
  12 iterations; re-measure with `SVSIM_SETTLE_STATS=1`). Release-corpus
  runtime dropped 3.5× (54.4 s → 15.5 s); e.g. `regfile_8x8` 4 → 19 steps/s,
  sap programs ~300 → ~1,500 steps/s, picorv32 ~250 steps/s.
- Since the second review's step 5, the frontend is the
  `frontend/sv_parser/` module directory (`module_structure`, `statements`,
  `expressions`, `literals`, `const_eval`, `loop_unroll`, `tests`); the
  largest production file in the crate is `module_structure.rs` at 1,151
  lines.
- The workspace `Cargo.toml` builds the `svsim` package at `opt-level = 2`
  even in dev/test profiles; unoptimized simulation is ~9x slower and made
  the full suite impractical (unit tests dropped from ~130s to ~32s).
- Four-state runtime values, four-state JSON/CLI boundaries, native four-state
  literals, and public/top-level `inout` are part of the verified baseline
  (`parts/testing/025-TopLevelInout`, `parts/sap2/sap2_inout_top`).
- `picorv32` compiles and its checked-in harness exercises a curated runtime
  subset. `sap2` and `sap3` are runnable corpora with program suites and
  focused bus-semantics regressions.
- Rendering is still intentionally deferred; the project is centered on
  structured compile and simulation results.
- The architectural review campaign is complete (all seven steps):
  [../../plans/completed/2026-07-06-architectural-review.md](../../plans/completed/2026-07-06-architectural-review.md).
  Highlights: green-corpus `cargo test` gate; `sim/` module split; four-state
  primitives in `logic_ops.rs`; one shared expression evaluator
  (`expr_eval.rs`) replacing the three divergent constant evaluators;
  parameter values resolved during elaboration; legacy ROM shim isolated and
  documented in `sim/legacy_rom.rs`.

## Commands Used

```text
cargo test
./test.sh
```

## Relationship To Older Docs

- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md) still describes
  the intended architecture and subset accurately at a high level.
- [../../plans/completed/plan-sap2-inout.md](../../plans/completed/plan-sap2-inout.md)
  is the completed four-state / `inout` / `sap2` campaign plan (moved from the
  repo root; its status snapshot predates the final public-`inout` unlock).
- [../../plans/completed/plan-sap3.md](../../plans/completed/plan-sap3.md) is
  the completed `sap3` plan (moved from the repo root).
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md)
  remains useful historical context but predates the four-state boundary flip,
  the runnable `sap2`/`sap3` corpora, and the corpus gate.

## Sources

- [../../AGENTS.md](../../AGENTS.md)
- [../../crates/svsim/tests/corpus_gate.rs](../../crates/svsim/tests/corpus_gate.rs)
- [../../test.sh](../../test.sh)
- [../../plans/completed/2026-07-06-architectural-review.md](../../plans/completed/2026-07-06-architectural-review.md)
- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md)
