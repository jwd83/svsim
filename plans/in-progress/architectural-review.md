# Architectural Review — svsim (second review)

*Reviewed 2026-07-06 against `main` @ `1b08d69`.*

This is the second architectural review, taken immediately after the completed
seven-step campaign recorded in
[2026-07-06-architectural-review.md](../completed/2026-07-06-architectural-review.md).
The structural debts that review targeted — the `sim.rs` god module, three
divergent constant evaluators, an unverifiable corpus, untruthful guidance
docs — are gone, and the layering that emerged (frontend → HIR → validate →
elaborate → sim, with one shared expression evaluator) is worth defending.
What remains expensive is different in kind: the **runtime evaluation model**
is the dominant carrying cost (a release build simulates an 8×8 register file
at 4 steps/second, and the test gate pays for it on every run), the
**parameterization story is split-brain** (per-instance parameter values exist
at elaboration, but shapes and unrolled loops silently freeze lowering-time
defaults), and the **negative corpus is not gated at all**. The recommended
order of attack: close the verification gap first (cheap), fence the
parameterization trap with diagnostics, then finish the runtime's own
half-completed migration to indexed frames — the machinery already exists and
is being bypassed — before splitting the last monolith (`sv_parser.rs`) and
threading spans through diagnostics.

## Snapshot

| Metric | Value |
| --- | --- |
| Rust source | 17,365 lines across 28 files (~11,650 production, ~5,715 test — 33%) |
| Largest code files (excl. inline tests) | `frontend/sv_parser.rs` 3,563 · `sim/session.rs` 956 · `test.rs` 752 · `validate.rs` 729 · `logic_value.rs` 648 · `compiler.rs` 603 |
| Test baseline | `cargo test`: **201/201 pass** (182 `svsim` unit, 9 corpus-gate, 10 CLI), ~2 min wall with the dev/test `opt-level = 2` override |
| Green corpus | 194 suites across 9 gated `parts/` dirs; release-build total 59 s (`./test.sh`) |
| Negative corpus | 6 suites in `parts/failing` — **not gated**; only the manual `test-fails.sh` runs them |
| Simulation speed (release, from `docs/tests/` `step_hz`) | picorv32 65–128 Hz · sap1–3 ~170–360 Hz · `regfile_8x8` **4 Hz** · `adder_cs_64bit` **1 Hz** |
| Docs | `wiki/` synced 2026-07-06; `docs/progress/` historical; `test.bat` stale; no root `README.md` |

Interpretation: verification is healthy and the code is well-layered, but
simulation speed is now the binding constraint — it sets the price of the test
gate, forced the picorv32 corpus to stay a "curated subset," and caps how much
real hardware the corpus can absorb. The slowest suites are not the biggest
designs: an 8×8 register file at 4 Hz in a release build is an algorithmic
signal, not a constant-factor one.

## Structurally sound elements

Do not regress these while fixing the findings below.

- **The compile pipeline and its ownership boundaries.** `sv-parser` types
  stop at `frontend/`; HIR is owned and serializable; `validate.rs` is
  validation-only (729 lines, down from 1,048); elaboration owns parameter
  resolution. Dependency direction is clean throughout — no layer reaches
  backward.
- **One shared expression evaluator.** `expr_eval.rs` (594 lines) is the only
  place expression semantics live; const contexts call the runtime evaluator
  with a params-only module and empty memories, so const and runtime semantics
  cannot diverge. Short-circuit `&&`/`||` is deliberate and load-bearing
  (picorv32 generate pruning). Any new operator goes here and nowhere else.
- **The corpus gate.** `crates/svsim/tests/corpus_gate.rs` makes `cargo test`
  the single verification command, errors on empty corpus dirs, and turned
  the committed reports into regenerable artifacts (`./test.sh`) rather than
  trusted ones. This is the safety net every step below relies on.
- **Four-state core with a two-state host boundary.** `LogicValue` inside,
  `BitValue` only at JSON/CLI edges, converters isolated in `sim/value.rs`.
  A locked decision from the first review; still correct.
- **The `sim/` module split and `logic_ops.rs` truth tables.** Six focused
  files with `pub(super)` seams, and the four-state primitives have direct
  truth-table tests. The split pattern (submodule dir, `use super::*;`, glob
  re-export) is proven and reusable — step 5 below reuses it verbatim.
- **Thin CLI.** `svsim-cli/main.rs` is 242 lines of clap-based wrapping over
  library APIs, exactly as `AGENTS.md` prescribes.

## Structural risks and costs

Ranked by ongoing development cost.

### 1. The runtime settle loop does quadratic work through string-keyed adapter tables

**Evidence.** `sim/session.rs:291` (`settle_module`): the iteration budget is
proportional to design size (assigns + procs + signals + children, summed
recursively), multiplied by 8; each iteration **clones the entire design frame**
(`frame.to_vec()`) and detects convergence by deep-comparing the whole frame
— while the `changed` flag that `settle_module_pass` already computes is
discarded (`let _changed = …`). Inside each pass, `build_instance_value_table`
(`sim/state.rs:341`) rebuilds a fresh `HashMap<String, Value>` — cloning every
port/signal/parameter name and value — once for the module's own evaluation
(`session.rs:375`) and **twice more per child per pass** (`session.rs:447,471`),
plus twice per step phase (`session.rs:498,513`). Evaluation then runs against
the string table and results are written back to the indexed frame. Measured
consequence: `regfile_8x8` at 4 steps/s and `adder_cs_64bit` at 1 step/s in a
**release** build; the two `regfile_8x8` suites alone are 22 s of the 59 s
release corpus.

**Consequence.** Cost per step grows roughly quadratically with design size.
The test gate pays ~2 minutes on every `cargo test`; picorv32 runs a curated
subset because full programs are impractical; every new corpus directory makes
the gate slower; larger designs (the obvious next milestones) are blocked.

**Fix direction.** This is a *half-finished migration, not a redesign*: the
indexed frame (`RuntimeObjectLayout`, `ObjectValue` frames) already exists as
the storage layer — the compute layer just never adopted it. Finish it: let
evaluation read/write the frame through a small resolver seam instead of
round-tripping through freshly allocated string tables, then replace
clone-and-deep-compare convergence with the change tracking the pass already
computes. The `step_hz` fields in `docs/tests/` reports are a built-in
before/after benchmark. Explicitly *not* needed: an event-driven scheduler,
dependency graphs, or any behavioral change — the fixpoint semantics stay.

### 2. Parameterization is split-brain: shapes and unrolled loops freeze defaults; expression values are per-instance

**Evidence.** HIR stores selects and ranges as concrete `usize`
(`hir.rs:210` — `BitSelect { index: usize }`, `PartSelect { msb, lsb: usize }`),
and elaboration derives object shapes from those frozen widths
(`elaborate.rs:252` uses `port.width()`). Procedural `for` loops are fully
unrolled at *lowering* time against the module's **default** parameter values
(`sv_parser.rs:1716`, `lower_for_loop_statement` → `const_eval_param_expr`
over `module.parameters`). But since the first campaign's step 6, parameter
*values* are resolved per instance at elaboration and used by runtime
expression evaluation. So an instance override affects expressions but not
widths, not memory shapes, and not unrolled loop bodies — a module whose loop
bound or port range depends on an overridden parameter silently simulates with
the default. Today only 3 corpus files use `#(` overrides (value-only), so the
trap is latent, not firing.

**Consequence.** A silent-wrong-answer class of bug sitting directly on the
path of corpus growth: importing any parameterized real-world core (the way
picorv32 was imported) can trip it, and the failure mode is wrong simulation
results, not an error.

**Fix direction.** Behavior-preserving fence first: during elaboration, reject
any instance parameter override whose parameter feeds a port/signal/memory
range or an unrolled loop bound, unless the override equals the lowering-time
default — with a diagnostic that names the parameter and the frozen construct.
Add negative-corpus cases. That converts silent-wrong into a hard error. The
full fix (ranges and bounds as `Expr` in HIR, resolved at elaboration) is a
future campaign once a use case demands it — do not build it speculatively.

### 3. The negative corpus is not gated

**Evidence.** `parts/failing/` holds 6 intentional-failure suites (bad
constant indices, duplicate instances, malformed JSON, missing modules…), but
`corpus_gate.rs` deliberately excludes it and nothing in `cargo test` runs it
— only the manual `test-fails.sh` does. The gate's own doc comment calls it
"the intentional negative corpus," yet a regression that makes
`duplicate_instance_names.sv` compile cleanly would pass CI silently.

**Consequence.** Half the diagnostic surface — everything `validate.rs` and
the frontend's `unsupported()` paths exist for — has no regression protection.
The first review's core lesson (the sap2 report that was silently sap1 data)
was exactly this class: verification that only runs when a human remembers.

**Fix direction.** Add a `corpus_failing_stays_red` test to `corpus_gate.rs`
asserting each suite in `parts/failing` fails compile/run as intended
(and that the failure message still mentions the expected construct, so the
diagnostics don't decay to generic errors). Retire `test-fails.sh` or reduce
it to a wrapper. Cheap — an afternoon — and it protects steps 2, 4, and 6.

### 4. `frontend/sv_parser.rs` is the last monolith, and it hides an elaboration engine

**Evidence.** 3,563 lines of production code (the next largest is 956), ~90
top-level functions in five natural clusters: module/port/parameter lowering
(~lines 99–1245), statement lowering (~1246–2550), expression/literal lowering
(~2551–3514), span/identifier plumbing (~3515–3562), and — the notable one —
a **loop-unrolling substitution engine** (`substitute_stmt_ident`,
`substitute_expr_ident`, `module_with_const_binding`, `expr_from_const_eval_value`,
~lines 1960–2140) that exists because HIR cannot represent loops or dynamic
selects, so the frontend must do elaboration-time work with lowering-time
information (this is the mechanism behind finding 2).

**Consequence.** Every language-feature addition lands in the same 3.5k-line
file; review diffs are noisy; the unroller's true nature (elaboration in
exile) is invisible, which is how finding 2 stayed hidden.

**Fix direction.** Mechanical split into `frontend/sv_parser/` using the
proven `sim/` pattern (`use super::*;` submodules, `pub(super)` seams, zero
call-site churn): `module_structure.rs`, `statements.rs`, `expressions.rs`,
`literals.rs`, and `loop_unroll.rs` — the last one named for what it is, with
a doc header stating it freezes lowering-time parameter defaults (cross-linking
finding 2's fence). No behavior change, no new IR.

### 5. Diagnostics are stringly typed and often span-less

**Evidence.** `diag.rs` is 31 lines: three bare-`String` error variants and a
`Diagnostic { message, span: Option<SourceSpan> }`. In `sv_parser.rs`, 58 of
163 `unsupported(…)` calls pass `span: None`; every `Error::Resolve` produced
by `width.rs` and `expr_eval.rs` carries no location at all; runtime
convergence failures name only the module. The harness reports failures per
suite, so a failure inside a 400-line imported core becomes
"unsupported: <construct>" with no line number.

**Consequence.** Corpus debugging happens by grep. As the corpus grows with
imported cores (the project's stated direction), time-to-locate dominates
time-to-fix.

**Fix direction.** Not a diagnostics framework. Thread the `Locate` that
lowering call sites already hold into the ~58 span-less `unsupported()` calls,
and give evaluator/width errors a construct-and-module prefix. Mechanical,
incremental, and each converted call site is immediately useful.

### Smaller frictions

- **`test.bat` is stale in exactly the class the first review fixed in
  `test.sh`**: it misses sap2/sap3/simple8 and invokes `svsim.exe`, a binary
  name the workspace doesn't produce (`svsim-cli`). Fix or delete; a broken
  parallel workflow is worse than none.
- **`wiki/architecture/workspace-map.md:34`** still credits `width.rs` with
  "shared shift/sign-extension helpers" deleted in the first campaign.
- **No root `README.md`** — `AGENTS.md` serves agents and contributors, but
  the repository has no human front door stating what svsim is.
- **Zero doc-tests** on a deliberately public library API (`lib.rs` re-exports
  ~40 types). A handful of doc examples on `Compiler` and `SimulationSession`
  would pin the intended usage.
- **`compiler.rs` is 68% inline tests** (1,303 of 1,906 lines, tempdir-heavy
  integration tests). Consistent with convention, but worth moving to
  `tests/` if the file grows again — not worth a step now.
- **`svsim-render`** remains a 5-line placeholder by explicit decision; keep
  deferring until rendering is actually scheduled.

## Recommended order of attack

1. **Gate the negative corpus.** Extend `corpus_gate.rs` with per-suite
   must-fail tests over `parts/failing` (asserting both failure and a stable
   diagnostic fragment); retire or thin `test-fails.sh`; fix or delete the
   stale `test.bat` in the same pass. This widens the safety net every later
   step stands on.

   *Done 2026-07-06*, in 3 commits (the review itself was committed as
   `0761ab8` by the user):
   - `d0f4982`: `corpus_failing_stays_red` in `corpus_gate.rs` — all 6
     `parts/failing` suites must fail: 5 asserted on stable suite-error
     fragments (constant memory OOB, duplicate instance, malformed JSON,
     missing child module, syntax error), 1 on an expectation mismatch
     (`outY`). A changed suite set or drifted diagnostic fails `cargo test`.
   - `0f8c0a2`: `test.bat` regenerates all nine reports (was missing
     sap2/sap3/simple8) with a gate-pointer header; `test-fails.sh`
     documented as a manual inspector and its Python-3.10-only `str | None`
     annotation fixed for the system Python 3.9 (the script was actually
     broken, discovered during verification); `parts/failing/README.md`
     dropped stale `ref/pysvsim.py` and four-directory-corpus text;
     `AGENTS.md` describes the must-fail gate.
   - (annotation commit): this note plus wiki sync (`log.md`,
     `testing/corpus-map.md`, `status/current-state.md`).

   Result: full suite 202/202 (182 unit + 10 gate + 10 CLI); the new gate
   test runs in ~0.02 s. Two deviations from the review text: `test.bat`'s
   `svsim.exe` claim was **wrong** — `svsim-cli` declares
   `[[bin]] name = "svsim"`, so only the missing directories were stale, and
   the script was fixed rather than deleted; `test-fails.sh` was kept (and
   repaired) as the only human-readable failure-report inspector rather than
   retired.
2. **Fence the parameterization trap.** In elaboration, reject instance
   parameter overrides that feed frozen constructs (port/signal/memory ranges,
   unrolled loop bounds) when they differ from the default, with a
   parameter-and-construct-naming diagnostic; add `parts/failing` cases (now
   gated by step 1). Document the freeze in `hir.rs` and the wiki.

   *Done 2026-07-06*, in 4 commits:
   - `30c8894`: the frontend records frozen parameters per module. Every
     lowering-time constant evaluation funnels through
     `const_eval_param_expr`, which now records the parameters each
     successful evaluation consumed, labeled by construct; the map lands
     serde-skipped on `ModuleSummary::frozen_parameters` (HIR JSON output
     unchanged). The freeze is documented on the field in `hir.rs`.
   - `3e75b57`: `elaborate_module_parameters` resolves pure defaults whenever
     an instance carries overrides and rejects any frozen parameter whose
     value would change — directly or through a dependent `localparam` —
     naming the parameter, module, instance, and frozen construct.
     Default-equal and runtime-only overrides stay allowed.
   - `206100e`: corpus coverage — `parts/failing` gains
     `param_override_frozen_range` and `param_override_frozen_loop` (gated
     must-fail, 8 suites), `parts/testing` gains `param_override_ok` (green
     proof of allowed overrides); gate and CLI tables updated.
   - (annotation commit): review/wiki sync and regenerated `docs/tests/`
     reports.

   Result: full suite 206/206 (186 unit + 10 gate + 10 CLI); green corpus
   195/195. Surprises: (a) the freezing surface is wider than this review
   listed — the frontend also constant-folds ordinary `if` conditions, so
   those freeze parameters too; the choke-point design covered them for
   free. (b) sv-parser mis-parses blocking assignments to bit-select lvalues
   at block starts as declarations (pre-existing subset limitation,
   discovered while writing tests; not addressed here). Deliberately out of
   scope: representing ranges/bounds as `Expr` in HIR for true per-instance
   shapes — wait for a real use case.
3. **Finish frame-native evaluation.** Remove the per-pass/per-child
   `build_instance_value_table` round-trips by letting `sim/eval.rs` and
   `expr_eval.rs` read signals through a small resolver seam over the indexed
   frame (const contexts keep the map-backed path). Record `step_hz`
   before/after from `./test.sh` reports.

   *Done 2026-07-06*, in 4 commits:
   - `caf3a41`: `ValueReader` trait in `expr_eval.rs`; `eval_expr` and
     `resolve_lvalue` read through it (HashMap impl keeps const contexts
     unchanged). Pure refactor.
   - `55b606d`: `FrameValues` reader over the indexed frame;
     `drive_child_inputs` evaluates through it (one table build per child
     per pass gone); comb blocks execute in place instead of
     clone-table-then-diff per block.
   - `3d02697`: the settle pass drops its full value table entirely —
     `OverlayValues` (copy-on-write overlay over the frame) serves reads,
     writes seed only touched names, and `commit_overlay_to_frame` applies
     the old sync's per-signal policy to dirty names plus precomputed
     per-module net specials (variable-storage-on-net → replace driver;
     procedural nets → stage-if-undriven — the load-bearing re-staging that
     keeps untouched nets from floating to Z). Child output sinks apply
     straight to the frame. `sync_instance_values_to_frame` survives only
     for `step_module` and the legacy ROM path.
   - (annotation commit): regenerated reports, review/wiki sync.

   Result: full suite 206/206 after every slice; `cargo test` wall time
   ~2 min → ~50 s. Release corpus (`./test.sh`) 54.4 s → 16.7 s. `step_hz`
   before → after: `regfile_8x8` 4 → 16, `adder_cs_64bit` 1 → 5, sap1
   395 → 1,577, sap2 307 → 1,418, sap3 298 → 1,316, picorv32 93 → 285.
   The remaining dominant cost is the full-frame clone-and-compare
   convergence check — step 4's target. Deliberately untouched:
   `step_module`'s per-step tables (once per step, not per settle
   iteration) and the legacy ROM path.
4. **Replace clone-and-compare convergence.** Use the change tracking
   `settle_module_pass` already computes instead of full-frame
   `to_vec()` + deep compare; revisit the ×8 iteration budget with measured
   iteration counts. Corpus gate (positive and now negative) is the oracle;
   expect this plus step 3 to move the slowest suites by an order of
   magnitude.

   *Done 2026-07-06*, in 3 commits:
   - `b8db4ff`: convergence is now `pass_changed | nets_changed` — no more
     per-iteration frame clone and deep compare. Key soundness point: the
     overlay-level flags from assigns and proc blocks are deliberately
     *discarded*, because a default-then-override sequence (`s = 0;
     s[i] = x;`) reports "changed" on every pass even at steady state;
     the sound signal is `commit_overlay_to_frame`'s comparison of each
     dirty name's final value against the frame.
   - `9e1a2c1`: measured the budget with an env-gated hook
     (`SVSIM_SETTLE_STATS=1`, kept for re-measurement): across 48,069
     settle calls, the deepest design converged in 12 iterations against
     budgets up to 558,880. The unfounded ×8 multiplier is gone; the bound
     is now budget + 1 confirming pass, floored at 16. The floor is
     load-bearing: an undriven pulled net (`tri1`) changes once before it
     can be confirmed stable — dropping the multiplier without it broke two
     tri-net unit tests, proof the ×8 had been silently covering a real
     +1-confirmation requirement.
   - (annotation commit): regenerated reports, review/wiki sync.

   Result: full suite 206/206. Release corpus 16.7 s → 15.5 s for step 4
   alone (after step 3 removed the table churn, the frame clone was no
   longer dominant). Combined steps 3+4 against the review baseline:
   54.4 s → 15.5 s (3.5×); `regfile_8x8` 4 → 19 step/s, `adder_cs_64bit`
   1 → 6, sap1 395 → 1,633, picorv32 93 → ~250. Honest shortfall: the
   predicted order-of-magnitude did not fully materialize — the slowest
   suites moved 4–6×, and the remaining cost is expression interpretation
   itself (per-operation `LogicValue` allocation), which is the runtime
   value-representation work this review deliberately left for a future
   campaign.
5. **Split `frontend/sv_parser.rs`** into a module directory using the `sim/`
   pattern — `module_structure.rs`, `statements.rs`, `expressions.rs`,
   `literals.rs`, `loop_unroll.rs` — naming the unroller for what it is and
   cross-referencing the step-2 fence in its doc header.

   *Done 2026-07-07*, in 6 commits:
   - `9bc1e5b`: `git mv` to `sv_parser/mod.rs`; 804-line inline test module
     extracted to `tests.rs`.
   - `b606a89`: `literals.rs` (251) and `const_eval.rs` (183 — the
     `const_eval_param_expr` choke point, usize funnels, and
     frozen-parameter recording, doc header pointing at the step-2 fence).
   - `5aeda2a`: `loop_unroll.rs` (468), doc header naming it elaboration
     work done at lowering time, stating the default-parameter freeze, and
     noting it is the code a future HIR loop representation would replace.
   - `4dc66d1`: `expressions.rs` (866).
   - `1473fb1`: `statements.rs` (744).
   - `f6a4f52`: `module_structure.rs` (1,151); `mod.rs` ends at 160 lines
     (doc header, `SvParserFrontend` entry points, span/identifier
     plumbing).

   Result: full suite 206/206 after every slice; zero call-site churn (the
   `sim/` pattern: `use super::*;` submodules, `pub(super)` items, private
   glob re-exports in `mod.rs`). One deviation from the review's five-file
   list: a sixth file, `const_eval.rs`, because the constant-evaluation
   funnels serve ranges and selects too, not just the unroller. The largest
   production file in the crate is now `module_structure.rs` at 1,151 lines
   (down from `sv_parser.rs` at 3,731 production lines).
6. **Thread spans through diagnostics.** Convert the ~58 span-less
   `unsupported()` sites in the (now split) frontend; prefix evaluator/width
   `Resolve` errors with module and construct context. Verify diagnostic
   texts against the gated negative corpus.

   *Done 2026-07-07*, in 3 commits:
   - `4593197`: `span_of_node` (first `Locate` leaf, evaluated only on the
     error path) threads spans through every `unsupported()` in
     `module_structure.rs` and `statements.rs`; slice-typed dimension sites
     use their first element.
   - `75a2724`: `expressions.rs` and the leaf helpers without source
     context (literal parsing, operator tables, identifier helpers,
     const-eval funnels) — the latter via `with_fallback_span`, which
     attaches spans at call-site boundaries so one wrap covers a whole
     family. Regression test pins that an unsupported construct's
     diagnostic carries its source line.
   - `2ee60ce`: `eval_expr`/`expr_width` `Resolve` errors name their module
     (and memory), covering the runtime side.

   Result: full suite 207/207 (new span regression test); gated
   negative-corpus fragments unaffected. Residual `span: None` sites are
   the lowered-`Expr`-level const-eval internals with no syntax node in
   scope — their errors now gain spans at the funnel boundaries instead.
7. **Truth-up and close.** Fix the stale `width.rs` row in the workspace map,
   add a root `README.md` pointing at `AGENTS.md`/wiki, add doc examples to
   `Compiler` and `SimulationSession`, re-run `./test.sh` to refresh committed
   reports (capturing the new `step_hz`), and move this review to
   `plans/completed/`.

## Closing assessment

The first campaign paid down the debts of *organization* — this codebase is
now well-layered, honestly documented, and verifiable with one command. What
remains are debts of *execution*: a runtime that does quadratic work through
string-keyed adapter tables it already has the machinery to avoid, a
parameterization seam that will silently mis-simulate the first real
parameterized core someone imports, and a diagnostic surface with no
regression protection. The dominant risk is the runtime's cost curve, because
it taxes every future test run and blocks corpus growth — but the best first
move is the cheap verification and fencing work (steps 1–2), which makes the
perf surgery in steps 3–4 safe to attempt. Expected payoff: a gate measured
in seconds rather than minutes, silent-wrong turned into hard errors, and no
production file over ~1,000 lines. None of this calls for new architecture —
the design is right; the remaining work is finishing what the design already
started.
