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
