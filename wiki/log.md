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

## [2026-07-06] lint | frozen-parameter fence (second architectural review, step 2)

- Verified `cargo test`: pass (`206/206` — `186` `svsim` unit tests, `10` corpus gate tests, `10` CLI tests).
- The frontend records, per module, which parameters were consumed by lowering-time constant evaluation (`ModuleSummary::frozen_parameters`, serde-skipped, labeled by construct). Every freezing site funnels through `const_eval_param_expr`, so the recording is a single choke point; the freezing surface proved wider than the review had listed (constant-folded ordinary `if` conditions freeze parameters too, not just ranges/loops/generate).
- Elaboration now rejects instance parameter overrides that would change a frozen parameter's value — directly or through a dependent `localparam` — naming the parameter, module, instance, and frozen construct. Default-equal overrides and runtime-only overrides stay allowed; picorv32's internal overrides pass the fence untouched.
- `parts/failing` gained two gated must-fail suites (`param_override_frozen_range`, `param_override_frozen_loop`; 8 suites total) and `parts/testing` gained `param_override_ok` (55 suites; green corpus now `195/195`). `docs/tests/` reports regenerated.

## [2026-07-06] lint | frame-native evaluation (second architectural review, step 3)

- Verified `cargo test`: pass (`206/206`); wall time dropped from roughly two minutes to roughly fifty seconds.
- The settle loop no longer round-trips through freshly built `HashMap<String, Value>` tables. Evaluation reads through the new `ValueReader` seam in `expr_eval.rs`: `FrameValues` reads straight from the indexed frame, and `OverlayValues` adds a copy-on-write overlay for the values a pass has written (blocking-assignment visibility preserved). Constant contexts keep the plain map path.
- The per-pass commit walks only dirty names plus precomputed per-module net specials (variable-storage-on-net objects replace their driver; procedurally driven nets restage if undriven — required because nets resolve from the current pass's drivers only). Child output sinks apply straight to the frame. `sync_instance_values_to_frame` remains for `step_module` and the legacy ROM shim.
- Release corpus (`./test.sh`) 54.4 s → 16.7 s. `step_hz` before → after: `regfile_8x8` 4 → 16, `adder_cs_64bit` 1 → 5, sap1 395 → 1,577, sap2 307 → 1,418, sap3 298 → 1,316, picorv32 93 → 285. The dominant remaining cost is the full-frame clone-and-compare convergence check (step 4's target).

## [2026-07-06] lint | tracked-change convergence (second architectural review, step 4)

- Verified `cargo test`: pass (`206/206`).
- The settle loop converges on `pass_changed | nets_changed` instead of cloning and deep-comparing the whole frame every iteration. Overlay-level change flags from assigns and proc blocks are deliberately discarded — a default-then-override sequence reports "changed" at steady state — and the sound signal is `commit_overlay_to_frame`'s comparison of each dirty name's final value against the frame.
- The iteration budget was measured with the new env-gated `SVSIM_SETTLE_STATS=1` hook: across `48,069` settle calls the deepest design converged in `12` iterations against budgets up to `558,880`. The historical ×8 multiplier is gone; the bound is budget + 1 confirming pass, floored at 16 (the floor covers undriven pulled nets — `tri1` — which change once before they can be confirmed stable).
- Release corpus 16.7 s → 15.5 s for step 4 alone; combined steps 3+4 versus the review baseline: 54.4 s → 15.5 s (3.5×), `regfile_8x8` 4 → 19 step/s, `adder_cs_64bit` 1 → 6, sap1 395 → 1,633. The remaining runtime cost is per-operation value allocation in expression evaluation — deferred to a future value-representation campaign, as the review planned.

## [2026-07-07] lint | sv_parser module split (second architectural review, step 5)

- Verified `cargo test`: pass (`206/206` after every slice).
- `crates/svsim/src/frontend/sv_parser.rs` (4,535 lines) is now the `sv_parser/` module directory: `mod.rs` (160 — public `SvParserFrontend` + plumbing), `module_structure.rs` (1,151), `expressions.rs` (866), `statements.rs` (744), `loop_unroll.rs` (468), `literals.rs` (251), `const_eval.rs` (183), `tests.rs` (802). Zero call-site churn via the proven `sim/` pattern.
- `loop_unroll.rs` is named for what it is — elaboration work done at lowering time — with a doc header stating the default-parameter freeze and cross-referencing the step-2 fence; `const_eval.rs` (a sixth file beyond the review's list) holds the `const_eval_param_expr` choke point and frozen-parameter recording.
- The largest production file in the crate is now `module_structure.rs` at 1,151 lines (was `sv_parser.rs` at 3,731 production lines).

## [2026-07-07] lint | diagnostics spans and campaign completion (second architectural review, steps 6-7)

- Verified `cargo test`: pass (`209/209` — `187` `svsim` unit tests, `10` corpus gate tests, `10` CLI tests, and the crate's first `2` doctests).
- Frontend `unsupported` diagnostics carry source spans: `span_of_node` (first `Locate` leaf, evaluated only on the error path) covers the lowering functions, and `with_fallback_span` attaches spans at call-site boundaries for leaf helpers without source context (literal parsing, operator tables, const-eval funnels). `eval_expr`/`expr_width` errors name their module.
- The repository has a root `README.md` (front door → `AGENTS.md`, wiki), and `Compiler`/`SimulationSession` carry runnable doc examples.
- The second architectural review campaign is complete (all seven steps); the review moved to `plans/completed/2026-07-07-architectural-review.md` (+ `.html`). Campaign totals against the 2026-07-06 baseline: negative corpus gated as must-fail, frozen-parameter overrides rejected with construct-naming diagnostics, release corpus 54.4 s → 15.5 s, `cargo test` ~2 min → ~50 s, largest production file 3,731 → 1,151 lines.
- Deferred by design for a future review: runtime value representation (per-operation `LogicValue` allocation is the residual perf cost) and `svsim-render`.

## [2026-07-07] perf | runtime value representation

- Verified `cargo test`: pass (`211/211` — `189` `svsim` unit tests, `10` corpus gate tests, `10` CLI tests, `2` doctests).
- Addressed the value-representation item the 2026-07-07 review deferred, profile-driven (`sample` on `regfile_8x8`), in four commits:
  - `31b8cc6`: `BitValue` limbs became an `Inline(u64)`/`Heap(Vec<u64>)` enum (invariant: heap only for ≥2 limbs, so derived `Eq`/`Hash` stay numeric). Single-limb values — nearly all runtime signal traffic — no longer allocate; hot ops take scalar fast paths. Was the malloc/free/memcpy that dominated the profile.
  - `818499f`: `LogicValue::coerced_to` returns a plain clone when the width already matches (the constructor invariant guarantees it), and `all_z` builds its mask directly instead of per-bit.
  - `1215d88`: an in-house Fx-style hasher (`fast_hash.rs`) replaced SipHash for the simulator's internal maps (`FrameValues`, overlay, `NetDriverTable`, elaborated parameters); `eval_expr`/`ValueReader` are generic over the hasher so const contexts and the public `BTreeMap` API are untouched.
  - `7591c52`: `into_coerced` (by-value) turns equal-width coercions on owned write/stage paths into moves rather than clones; `LogicBits::clone` was the top profile entry.
- Release corpus `15.5 s → 8.4 s` (`1.85×`); `regfile_8x8` 19 → 45 steps/s, picorv32 ~223 → ~469, sap programs ~1,500 → ~2,800. Combined with the review's steps 3–4, `regfile_8x8` is ~11× faster than the first review's baseline. The profile is now flat (no dominant cost); the next candidate — limb-parallel four-state bitwise ops in `logic_bitwise_binary` — was left as a smaller, higher-risk gain.
