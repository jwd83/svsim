# Log

## [2026-04-08] bootstrap | Initial wiki scaffold

- Created the initial wiki structure under `wiki/` with a schema, index, log, overview, architecture pages, testing notes, port case studies, a roadmap page, and a raw-source map.
- Built the first pass primarily from `AGENTS.md`, `docs/rust-port-plan.md`, `docs/sap1-port-compromises.md`, `docs/progress/progress-report-2026-03-20.md`, `parts/*/README.md`, and the `crates/svsim` Rust modules.
- Verified `cargo test`: pass (`131` `svsim` tests and `8` CLI tests; no failures).
- Verified `cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i`: pass (`136/136`).
- Verified `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`).
- Verified `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`166/166`).

## [2026-04-12] lint | four-state and sap2 wiki sync

- Reviewed the recent four-state / internal-`inout` / `sap2` work against `plan.md`, the Rust sources, and the checked-in corpus docs.
- Verified `cargo test`: pass (`168` `svsim` tests and `10` CLI tests; no failures).
- Verified `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i`: pass (`155/155`).
- Verified `cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32`: pass (`13/13`).
- Verified `cargo run -q -p svsim-cli -- --json-test-dir parts/sap2`: pass (`7/7`).
- Updated the status, architecture, roadmap, and corpus pages to match the verified April 12 state and added dedicated `sap2` and milestone pages.

## [2026-07-06] lint | corpus gate and doc truth-up (architectural review steps 1-2)

- Verified `cargo test`: pass (`187/187` — `168` `svsim` unit tests, `9` new green-corpus gate tests in `crates/svsim/tests/corpus_gate.rs`, `10` CLI tests).
- Recorded that `test.sh` had been writing sap1's results into `report-parts-sap2.json` and omitting `parts/sap3` entirely; the script is fixed and all `docs/tests/` reports were regenerated (sap2 truly `9/9`, sap3 `4/4` added).
- Moved root plan files into the plans lifecycle: `plan.md` → `plans/completed/plan-sap2-inout.md`, `plan-sap3.done.md` → `plans/completed/plan-sap3.md`; updated wiki links accordingly (append-only history entries left as written).
- Refreshed `status/current-state.md` (2026-07-06 snapshot), `architecture/workspace-map.md` (dropped the absent `ref/`, added `plans/` and the corpus gate), `overview.md`, `index.md`, `sources/source-map.md`, and marked the `inout`/`sap2` milestone page complete (public top-level `inout` is supported).
- AGENTS.md truth-up in the same pass: removed stale `ref/` and "no usable Git history" claims, documented the corpus gate and the dev/test `opt-level = 2` override for `svsim`.

## [2026-07-06] lint | sim module split and logic_ops extraction (architectural review steps 3-4)

- Verified `cargo test`: pass (`201/201` — `182` `svsim` unit tests, `9` corpus gate tests, `10` CLI tests).
- `crates/svsim/src/sim.rs` (4,970 lines) is now the `crates/svsim/src/sim/` module directory: `mod.rs` (root), `session.rs`, `eval.rs`, `state.rs`, `memory.rs`, `value.rs`, `tests.rs`; wiki links updated from the old `sim.rs` path.
- Four-state primitives (bit truth tables, reductions, slices, sign extension) moved from the sim runtime into crate-private `crates/svsim/src/logic_ops.rs` with 14 direct truth-table unit tests; `sim/eval.rs` is now a consumer.
- Updated `architecture/workspace-map.md`, `sources/source-map.md`, and `status/current-state.md` to match.

## [2026-07-06] lint | evaluator consolidation and campaign completion (architectural review steps 5-7)

- Verified `cargo test`: pass (`201/201` — `182` `svsim` unit tests, `9` corpus gate tests, `10` CLI tests).
- The three constant evaluators (validate's `ConstValue`, the frontend's `ConstEvalValue`, and runtime parameter evaluation) are consolidated onto one shared evaluator in `crates/svsim/src/expr_eval.rs`; net ~700 lines removed. The shared evaluator gained short-circuit `&&`/`||` (the historical frontend behavior picorv32's generate pruning relies on).
- Parameter values are now resolved during elaboration and carried on `ElaboratedInstance`; `validate.rs` is validation-only (1,048 → 729 lines).
- The legacy ROM shim is isolated in `crates/svsim/src/sim/legacy_rom.rs` with its `rom_<stem>` naming contract documented as do-not-extend.
- The architectural review campaign is complete; the review moved to `plans/completed/2026-07-06-architectural-review.md` and wiki links were updated (workspace map, source map, overview, current state).

## [2026-07-06] lint | negative corpus gated (second architectural review, step 1)

- Verified `cargo test`: pass (`202/202` — `182` `svsim` unit tests, `10` corpus gate tests including the new `corpus_failing_stays_red`, `10` CLI tests).
- A second architectural review was authored against the post-campaign baseline (`plans/in-progress/architectural-review.md`, committed as `0761ab8`); step 1 of its plan is complete.
- `parts/failing` (6 suites) is now gated as must-fail: 5 suites asserted on stable suite-error fragments, 1 on an expectation mismatch (`outY`); a changed suite set or drifted diagnostic fails `cargo test`.
- `test.bat` now regenerates all nine `docs/tests/` reports (it was missing sap2/sap3/simple8 — same drift class as the sap1-into-sap2 bug the first review caught in `test.sh`). Its `svsim.exe` invocation was correct all along: `svsim-cli` declares `[[bin]] name = "svsim"`.
- `test-fails.sh` is documented as a manual failure-report inspector (not a gate) and was actually broken on the system Python 3.9 (`str | None` annotation); fixed.
- `parts/failing/README.md` dropped stale `ref/pysvsim.py` and four-directory-corpus text; `AGENTS.md` and `wiki/testing/corpus-map.md` now describe the must-fail gate.
