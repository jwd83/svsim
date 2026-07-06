# Architectural Review — svsim

*Reviewed 2026-07-06 against `main` @ `94e5ef5`.*

The Rust rewrite is in good structural health where it matters most: the compile
pipeline (frontend → HIR → validate → elaborate → design) has clean, owned
boundaries, the CLI is genuinely thin, and behavior is pinned by a large
corpus-driven compatibility suite. The structural cost is concentrated in two
places: `sim.rs` has become a ~5,000-line god module absorbing every runtime
concern, and the verification story that the project's "all green" claims rest
on is quietly broken (`test.sh` never actually tests `parts/sap2`, and
`parts/sap3` is untested by any script). The recommended order of attack is:
fix verification first so every later change is provable, then split `sim.rs`
along its existing seams, then consolidate the three parallel expression
evaluators. Related plans: [`plan-sap2-inout.md`](../completed/plan-sap2-inout.md)
(sap2/inout campaign, landed) and [`plan-sap3.md`](../completed/plan-sap3.md)
(completed) — both moved from the repo root into the plans lifecycle in step 2.

## Snapshot

| Metric | Value |
| --- | --- |
| Workspace | 3 crates: `svsim` (core), `svsim-cli` (thin wrapper), `svsim-render` (5-line placeholder) |
| Rust source | ~17,700 lines total; core crate ~16,900 |
| Largest files | `sim.rs` 4,970 (3,059 impl + 1,911 inline tests); `frontend/sv_parser.rs` 4,669 (3,939 + 730); `compiler.rs` 1,906; `test.rs` 1,402; `validate.rs` 1,048 |
| Unit tests | 168 `#[test]` in core + 10 CLI integration tests |
| Test baseline | `cargo test` (debug): **178/178 passing** — but slow: core unit tests ~130s, CLI integration ~92s (one corpus test alone exceeds 60s) |
| Compatibility corpus | 8 green part dirs (~190 JSON suites), 1 negative dir (`parts/failing`, 6 suites) |
| Corpus reports | `docs/tests/report-parts-*.json` all show 100% pass — **but the sap2 report contains sap1's results** (test.sh bug) |
| Dependencies | clap, rayon, serde/serde_json, sv-parser, thiserror — small and all used |
| Docs | AGENTS.md + maintained `wiki/` (architecture, status, roadmap) + `docs/` progress reports |

Interpretation: test coverage is genuinely strong for a simulator at this
stage — the risk is not missing tests but that the shell-script layer that
runs the corpus and publishes "green" reports has rotted without anything
noticing, which is exactly the failure mode committed generated reports invite.
A secondary cost: the debug-mode suite takes ~4 minutes wall, because unit
tests inside `sim.rs`/`test.rs` execute whole corpus simulations — the corpus
gate proposed below should run the heavy suites in release or a tiered mode.

## Structurally sound elements

1. **Owned HIR boundary.** `sv-parser` types never escape
   `crates/svsim/src/frontend/` (verified by grep across the workspace). The
   rest of the system programs against `hir.rs` (474 lines, plain data). This
   is the single most load-bearing decision in the codebase: it makes the
   parser swappable and keeps the 4,700-line lowering file's complexity from
   contaminating the runtime. Do not let convenience imports breach it.

2. **Library-first, thin CLI.** `svsim-cli/src/main.rs` is 242 lines of clap
   dispatch over library entry points; the 521-line integration test drives
   the binary end-to-end. No compiler or simulator logic lives in the CLI.

3. **Corpus-driven compatibility testing.** JSON expectation suites beside the
   SystemVerilog sources under `parts/`, with a deliberate green/negative split
   (`parts/failing` isolated), give behavior-preservation work a real oracle.
   The four-state migration and the inout campaign were both landed against
   this corpus without breaking it — proof the mechanism works.

4. **Typed value layers with explicit escape hatches.** The locked decision to
   keep `BitValue` (2-state, host-facing) separate from `LogicValue` (4-state,
   HDL runtime) is documented in `plan.md`, and the `_2state` wrapper APIs
   preserve legacy fail-on-x/z behavior instead of silently changing it.

5. **Maintained knowledge layer.** `wiki/architecture/` (workspace map,
   compiler pipeline, runtime-and-state) is accurate at the module level and
   materially shortens onboarding. AGENTS.md rules (owned HIR, library-first,
   explicit configuration over naming magic) match the code's actual shape —
   with the exceptions noted below.

6. **Deliberate deferral of rendering.** `svsim-render` is a placeholder on
   purpose. Nothing in this review should be read as a reason to build it out;
   resist the pull to "finish" it before the runtime needs it.

## Structural risks and costs

### 1. Verification scripts are broken and the published reports are stale artifacts

Evidence: `test.sh:10` runs `--json-test-dir parts/sap1` a second time and
writes the output to `docs/tests/report-parts-sap2.json` — `parts/sap2`
(9 JSON suites) is never executed by the script, and its committed "6/6 green"
report is actually sap1 data. `parts/sap3` (4 suites) appears in no script at
all. Three divergent runners exist (`test.sh`, `test-fails.sh`, `test.bat`),
and `docs/tests/*.json` are generated artifacts committed to git with no
mechanism to detect drift.

Consequence: the project's core invariant — "the green corpus is green" — is
currently unverifiable by any single command, and has silently regressed at
least once (sap2). Every future slice lands with false confidence.

Fix direction: make the green corpus a `cargo test` gate (an integration test
in the core crate that walks the green `parts/` dirs via the existing
`run_json_test_dir` API and asserts all-pass). Fix or retire the shell
scripts; either regenerate `docs/tests/` reports as part of the gate or stop
committing them. No new infrastructure needed — the library APIs already do
all of this.

### 2. `sim.rs` is a god module absorbing every runtime concern

Evidence: 3,059 implementation lines (plus 1,911 inline test lines) containing
at least seven distinguishable responsibilities: the public
`SimulationSession` API (~280 lines); hierarchical state instantiation
(`instantiate_module_state`, ~160 lines); the settle/step scheduler
(`settle_module_pass`, `step_module`); a full four-state expression
interpreter (`eval_expr`, ~220 lines, plus ~500 lines of logic-bit primitives
at sim.rs:2043–2316); lvalue resolution and net-driver staging (~400 lines);
`$readmem`-style memory-file parsing (sim.rs:403–518); the legacy ROM shim
(sim.rs:1816–1921); and per-instance parameter elaboration
(`elaborate_module_parameters`, sim.rs:1921).

Consequence: every runtime feature, bug fix, and slice lands in the same file
— review diffs concentrate there (see recent slice commits), unrelated changes
collide, and the interpreter primitives can't be unit-tested where they live.
This is the dominant ongoing tax on development.

Fix direction: split into a `sim/` module directory along the seams that
already exist — the file is mostly free functions with clean signatures, so
this is cut-and-paste: `value.rs` (`Value`/`ObjectValue` + logic primitives),
`eval.rs` (expression interpreter + lvalue resolution), `state.rs` (module
state instantiation + bindings), `memory.rs` (memory files, `MemoryState`,
legacy ROM), `session.rs` (public API + settle/step). Move inline tests with
their subjects. No trait abstractions, no new crates — AGENTS.md already says
to prefer modules over crates.

### 3. Three parallel expression evaluators with divergence risk

Evidence: (a) `validate.rs::const_eval_expr` + `ConstValue` — ~400 lines of
2-state constant evaluation (validate.rs:648–1048); (b)
`sim.rs::eval_expr` — the four-state runtime interpreter; (c)
`frontend/sv_parser.rs::lower_constant_*` — parse-time constant folding
(sv_parser.rs:432–614). Parameter values are additionally evaluated at
session-build time by `sim.rs::elaborate_module_parameters` using evaluator (b).

Consequence: every new operator or width rule must be implemented and kept
consistent in up to three places; a constant expression can legally fold
differently at compile time than it simulates at runtime, and nothing tests
the evaluators against each other.

Fix direction: consolidate on one HIR-level constant evaluator (a good home:
`width.rs`'s neighborhood or a new `const_eval.rs`) shared by validation,
frontend folding, and parameter elaboration. The runtime interpreter stays
separate (it needs signals/memories), but should share the primitive bit ops
(see risk 4).

### 4. Four-state primitives live in the interpreter instead of the value layer

Evidence: `logic_bit_not/and/or/xor`, reductions, shifts, comparisons, slices,
sign-extension — ~500 lines at sim.rs:2043–2316 — operate purely on
`LogicBit`/`LogicValue`/`Value` yet live in `sim.rs`, while `logic_value.rs`
(699 lines, well-tested) exposes only construction/inspection.

Consequence: the truth tables that define simulator correctness are tested
only indirectly through whole-simulation tests; the const evaluator (risk 3)
can't reuse them, which is part of why it reimplements arithmetic on
`BitValue`.

Fix direction: move the primitives into `logic_value.rs` (or a sibling
`logic_ops.rs`) with direct unit tests per truth table. Mechanical,
behavior-preserving, and it shrinks `sim.rs` as a side effect.

### 5. Elaboration responsibilities are split across three files

Evidence: `elaborate.rs` (357 lines) computes runtime object *shapes* but not
parameter *values*; values are computed lazily in `sim.rs` per session build;
`validate.rs` owns `resolve_legacy_rom_data_path` (path resolution, not
validation) which `sim.rs` imports. The wiki describes `elaborate.rs` as "the"
elaboration layer, which no longer matches.

Consequence: unclear ownership — a parameter bug can plausibly live in three
files; elaboration can't be inspected or cached independently of a live
simulation session.

Fix direction: move parameter evaluation into `elaborate.rs` so
`ElaboratedDesign` carries resolved parameter values that `sim.rs` consumes;
relocate ROM path resolution out of `validate.rs`. This is a re-homing, not a
redesign.

### 6. String-keyed, clone-heavy runtime state (accepted for now)

Evidence: `ModuleState` keys parameters, signals, memories, and clock state by
`HashMap<String, …>`; port drivers carry cloned `Expr`s;
`build_instance_value_table`/`sync_instance_values_to_frame`
(sim.rs:1212–1344) maintain a second, name-keyed view of the object frame that
must be re-synced every settle pass.

Consequence: two representations of runtime state must be kept coherent, and
per-step hashing/cloning sets a hard performance ceiling. This is the wall
`picorv32` simulation will hit (it currently compiles but does not simulate —
a feature gap today, a performance gap tomorrow).

Fix direction: none yet — explicitly deferred. When picorv32-scale simulation
becomes a goal, extend elaboration to assign numeric object ids/interned names
ahead of time. Do not attempt this rewrite as part of the current campaign; it
would destabilize the corpus for no present-day gain.

### Smaller frictions

- **Doc drift:** AGENTS.md and the wiki reference a `ref/` Python oracle that
  is absent from this checkout; AGENTS.md claims "no usable Git history" while
  the repo has a meaningful history; `docs/progress/` stops at 2026-03-20.
- **Plan scatter:** active/completed plans live at the repo root (`plan.md`,
  `plan-sap3.done.md`), in `docs/` (`rust-port-plan.md`), and in
  `wiki/roadmap/` — with no single lifecycle. This review adopts
  `plans/in-progress/` / `plans/completed/`.
- **Legacy ROM magic naming** contradicts AGENTS.md's "explicit configuration
  over hidden naming conventions"; it's a compat shim, fine to keep, but it
  should be isolated and documented as legacy (covered by risks 2/5).
- `parts/roms/` is empty; `test.bat` appears Windows-stale.

## Recommended order of attack

1. **Make the green corpus a `cargo test` gate and repair the scripts.** Add a
   core-crate integration test that runs every green `parts/` dir through
   `run_json_test_dir` and asserts all-pass (sap2's 9 suites and sap3's 4
   included). Fix `test.sh` (sap2 typo, add sap3) or reduce it to a thin
   wrapper over the gate; regenerate the committed `docs/tests/` reports or
   remove them in favor of generated-on-demand output.

   *Done 2026-07-06*, in 2 commits:
   - `7a04e88`: added `crates/svsim/tests/corpus_gate.rs` — 9 tests, one per
     green `parts/` dir via the public `run_json_test_dir` API; missing or
     emptied dirs fail rather than passing vacuously. Also added
     `opt-level = 2` profile overrides for the `svsim` package in dev/test
     builds: unoptimized simulation is ~9x slower (sap3 alone ~100s debug vs
     ~11s release), which would have made the gate a 10+ minute run.
   - `7d387c1`: fixed `test.sh` (sap2 typo, added missing sap3 run,
     `set -euo pipefail`, header pointing at the cargo gate) and regenerated
     `docs/tests/` — sap2 now reports its real 9/9 suites (was sap1 data),
     `report-parts-sap3.json` (4/4) is new.

   Result: full `cargo test` = 187/187 passing (168 unit + 9 gate + 10 CLI)
   in ~115s wall — faster than the previous 178-test baseline (~4 min pure
   test time) despite the added gate, thanks to the profile override (unit
   tests 130s→32s, CLI integration 92s→11s). Deviations: the snapshot's
   "8 green part dirs" undercounted — there are 9. Deliberately out of scope:
   `test.bat` remains stale (smaller frictions / step 2), and the committed
   reports were kept-and-regenerated rather than removed.
2. **Truth up the guidance docs.** Fix AGENTS.md (`ref/` absence, git-history
   note, test commands), refresh `wiki/architecture/workspace-map.md` and
   `wiki/status/current-state.md` against post-sap3 reality, and move root
   plan files into the `plans/` lifecycle (`plan-sap3.done.md` →
   `plans/completed/`).

   *Done 2026-07-06*, in 1 commit:
   - AGENTS.md: removed the absent-`ref/` references and the "no usable Git
     history" claim; documented the 9-dir gated green corpus, the corpus gate
     as the gating check, `test.sh`'s report-regeneration role, and the
     dev/test `opt-level = 2` override.
   - Plans lifecycle: `plan.md` → `plans/completed/plan-sap2-inout.md`,
     `plan-sap3.done.md` → `plans/completed/plan-sap3.md` (rewrote the 35
     relative links inside the latter for the new location).
   - Wiki: refreshed `status/current-state.md` (2026-07-06 verified snapshot),
     `architecture/workspace-map.md`, `overview.md`, `index.md`,
     `testing/corpus-map.md`, `sources/source-map.md`,
     `architecture/compiler-pipeline.md`, `ports/sap2.md`, and the
     `inout`/`sap2` milestone page (dated completion update); appended a
     `log.md` entry per wiki convention.

   Result: docs-only change; test baseline unchanged from step 1 (187/187).
   Surprises: the wiki was *ahead* of this review in one place — public
   top-level `inout` is supported (verified: `025-TopLevelInout` and
   `sap2_inout_top` sit in the gated corpus, and `validate.rs` no longer
   rejects it), so `plan.md`'s campaign was complete rather than "mostly
   landed" and moved to `completed/`, not `in-progress/`. Also fixed three
   dangling links to a renamed `plan-next.md` in `roadmap/open-edges.md`.
   Deliberately untouched: `docs/progress/*` and prior `wiki/log.md` entries
   (historical/append-only), and `test.bat`.
3. **Split `sim.rs` into a `sim/` module directory** along existing seams:
   `value.rs`, `eval.rs`, `state.rs`, `memory.rs`, `session.rs`, moving inline
   tests with their subjects. Public API (`SimulationSession` re-export)
   unchanged; behavior-preserving by construction, verified by the step-1 gate.
4. **Move the four-state primitive operations into the value layer**
   (`logic_value.rs` or `logic_ops.rs`) with direct truth-table unit tests;
   `sim/eval.rs` becomes a consumer.
5. **Consolidate constant evaluation into one shared HIR const-evaluator**
   used by `validate.rs`, frontend constant folding, and parameter
   elaboration; delete `ConstValue` and the frontend's private folding once
   ported, with regression tests pinning today's accepted/rejected corpus
   behavior.
6. **Re-home elaboration:** move `elaborate_module_parameters` from `sim.rs`
   into `elaborate.rs` (resolved values on `ElaboratedDesign`), and move
   `resolve_legacy_rom_data_path` out of `validate.rs` next to the ROM shim.
7. **Isolate the legacy ROM shim** in its own module with a short doc header
   stating the naming contract and its legacy status.

Deferred by design: runtime interning/performance work (risk 6) and any
build-out of `svsim-render` — both wait until their milestones are actually
scheduled.

## Closing assessment

The dominant risk is concentration: one file owns the runtime, and one broken
shell script owns the credibility of the compatibility suite. The best
leverage point is step 1 — a single `cargo test` that truthfully proves the
corpus green — because it converts every subsequent refactor from "careful"
to "mechanical." After the split and consolidation steps, the expected payoff
is that new language features touch one evaluator and one runtime module
instead of three evaluators inside a 5,000-line file, and that "all green"
means what it says.
