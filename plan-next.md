# Next Plan: Finish The SAP Port Story

This plan picks up where [`plan.md`](./plan.md) left off. The four-state /
internal-`inout` / `sap2` milestone has effectively landed; the question now is
what is the next concrete, defensible step that moves the SAP-1 redesign and
SAP-2 port toward "completed or meaningfully moved" without overreaching.

## Verified Snapshot (2026-04-16)

Counts come from `cargo test` and `cargo run -q -p svsim-cli -- --json-test-dir
<dir>` runs done today against the current `main` checkout.

| Surface                                                  | Pass / Total |
| -------------------------------------------------------- | ------------ |
| `cargo test` (svsim core)                                | 168 / 168    |
| `cargo test` (CLI)                                       | 10 / 10      |
| Green corpus (`basic` + `testing` + `overture` + `rv32i`)| 155 / 155    |
| `parts/picorv32`                                         | 13 / 13      |
| `parts/sap2`                                             | 7 / 7        |
| `parts/sap1`                                             | 6 / 6        |
| `parts/simple8`                                          | 5 / 5        |
| Compile-only across all 8 tracked dirs                   | 146 / 146    |

Combined executable surface (green + auxiliary): **186 / 186**.

### Wiki vs Code Drift

- [`wiki/status/current-state.md`](./wiki/status/current-state.md) reports
  `parts/testing` at `50/50`. Actual today is `52/52` (`022-FourStateControl`,
  `023-FourStateBoundary`, `parameter_defaults.json`, etc.). The wiki snapshot
  is from 2026-04-12 and is mildly stale on counts but otherwise accurate.
- The wiki does not currently record the `parts/sap1` `6/6` and `parts/simple8`
  `5/5` runtime baselines.
- All other architectural claims in the wiki match the code (four-state runtime
  values, elaborated structural runtime, internal whole-net `inout`, public
  top-level `inout` still rejected at
  [`crates/svsim/src/validate.rs:207-214`](./crates/svsim/src/validate.rs)).

### Plan Unlock Bar Status

The `plan.md` "Public `inout` Unlock Bar" lists six conditions. All six now
read as satisfied:

1. Elaboration owns typed ports, net kinds, storage kinds, instance bindings -
   live in [`crates/svsim/src/elaborate.rs`](./crates/svsim/src/elaborate.rs).
2. Per-bit net resolution and shared structural connectivity - live in
   [`crates/svsim/src/net_resolve.rs`](./crates/svsim/src/net_resolve.rs) and
   the structural rewiring inside `sim.rs`.
3. Four-state expression and control semantics - live in `sim.rs`.
4. Public API, CLI, JSON, traces, reports round-trip literal `x`/`z` and `?` -
   covered by `LogicValue` parsing/formatting and `LogicPattern` matching.
5. Existing green corpora pass - verified above.
6. `parts/sap2` and focused `inout` / floating / contention tests pass -
   verified above (`7/7`).

The remaining lock is policy, not capability: top-level `inout` is still
rejected by `validate_supported_port_directions` and the sim's child-binding
path also returns `Unsupported` for `Inout` when no parent net alias exists
([`crates/svsim/src/sim.rs:734`](./crates/svsim/src/sim.rs)).

## Where SAP-2 Stands Today

[`parts/sap2/sap2.sv`](./parts/sap2/sap2.sv) keeps the same harness contract as
SAP-1 but rewires the *output* side of every bus producer through real `inout`
leaf drivers. The remaining gaps to "feels like the original Ben Eater shared
bus" are:

- **Per-tile bus participation**: `register a`, `register b`, `registerpc pc`,
  and `register instr` still consume the bus as plain `input wire [7:0] bus`
  and rely on a sibling `bus_driver reg_a_bus` / `bus_driver pc_bus` to put
  them on the bus. The original "register tile" shape has both `en_write` and
  `en_read` on the register itself, with the tristate driver inside the tile.
- **Idiomatic high-Z**: the bus driver currently has to use a separately
  declared, never-driven `wire [7:0] float_bus` to obtain `z`, instead of
  writing `assign bus = en_read ? value : 8'bz;`. That is because
  [`parse_based_value`](./crates/svsim/src/frontend/sv_parser.rs:3733) in the
  frontend silently coerces `x` and `z` digits to `0` via
  `coerce_unknown_digits_to_zero`. Today's `NumericLiteral.bits` is a 2-state
  `BitValue`, so `8'bz` lowers as `8'b0`.
- **Public top-level `inout`**: every external driver (the test harness) still
  has to come in through a regular `input` plus an internal `bus_driver`. There
  is no way for a JSON suite to express "this cycle, external driver releases
  the bus" except by gating an `en_read_external` input.

## Recommended Next Defensible Step

Land **native four-state literal lowering** as the next slice. Of the open
gaps, this one is the smallest change with the highest leverage:

- It removes the `float_bus` workaround from
  [`parts/sap2/sap2.sv`](./parts/sap2/sap2.sv) and
  [`parts/sap2/sap2_bus_semantics.sv`](./parts/sap2/sap2_bus_semantics.sv) so
  the source matches what a real Verilog author would write.
- It is a near-prerequisite for any honest top-level `inout` story, because
  external testbenches conventionally release a bus with `8'bz`.
- It is contained: the change set is the literal lowering path, the const
  evaluator's literal arm, and one or two focused tests. It does not touch the
  net resolver, elaboration, or the public boundary.

### Slice 1: Native Four-State Literals

**Status: landed 2026-04-16.** `NumericLiteral.bits` is now `LogicBits`;
`parse_based_value` lowers `x`/`X`/`z`/`Z`/`?` digits with IEEE 1800 §5.7.1
x/z-fill on width extension; `coerce_unknown_digits_to_zero` is gone.
`parts/sap2/sap2.sv` and `parts/sap2/sap2_bus_semantics.sv` now use
`8'bz` / `1'bz` directly. Added `parts/testing/024-FourStateLiterals.{sv,json}`.
Verified: `cargo test --workspace` 178/178; `parts/basic+testing+overture+rv32i`
156/156 (includes new 024); `parts/picorv32+sap2+simple8` 25/25;
`parts/sap1` 6/6.

Scope:

1. Replace `NumericLiteral.bits: BitValue` with a `LogicBits`-backed payload
   (or add a parallel four-state variant), and update the four call sites in
   `hir.rs`, `validate.rs`, `sim.rs`, and `frontend/sv_parser.rs`.
2. In `parse_based_value`, drop `coerce_unknown_digits_to_zero` and lower `x`,
   `X`, `z`, `Z`, and `?` digits into `LogicBit::X` / `LogicBit::Z` /
   wildcard. Preserve the existing radix coverage (`'b`, `'o`, `'h`, `'d`).
3. In `sim.rs::value_from_literal`, build a `Value::from_logic` directly from
   the literal's `LogicValue` instead of constructing through `BitValue`.
4. In `validate.rs::const_value_from_literal` and `const_eval_expr`, treat
   non-two-state literals as non-const for now (returning `None` from
   `const_eval_expr`). Width validation still works since `minimum_width` only
   needs widths.
5. Keep the unbased unsized `'x` / `'z` arm, but flow them through the new
   four-state path so they are no longer silently zeroed.
6. Add a focused `parts/testing/024-FourStateLiterals.{sv,json}` suite that
   pins:
   - `8'bx` and `8'bz` propagate to top-level outputs as `x` / `z`.
   - `4'bxxxx` and `4'bzzzz` round-trip through JSON harness expectations.
   - Width-extended literals (`8'bz` widened to `16` bits) extend correctly.
7. Rewrite `parts/sap2/sap2.sv` and `parts/sap2/sap2_bus_semantics.sv` to use
   `assign bus = en_read ? value : 8'bz;` (and the 1-bit form) and delete the
   `float_bus` placeholder wires.

Exit:

- The full green corpus remains at 155/155 and `cargo test` stays at 178/178.
- `parts/sap2` stays at 7/7 with the rewritten driver source.
- `parts/testing/024-FourStateLiterals.json` is added and passes.

### Slice 2: SAP-2 Register-Tile Partitioning

**Status: landed 2026-04-16.** `register_tile`, `register_pc_tile`, and
`register_instr_tile` now live in [`parts/sap2/sap2.sv`](./parts/sap2/sap2.sv)
and absorb the former `reg_a_bus`, `pc_bus`, and `instr_bus` driver siblings.
`machine` now instantiates 3 `bus_driver` instances (external, alu, mem) instead
of 6. `register` is still used for the write-only `b` and `out_r` (which never
drive the bus). Added [`parts/sap2/sap2_register_tile.{sv,json}`](./parts/sap2)
covering en_write capture, en_read drive, floating bus, and contention.
Verified: `cargo test` 178/178; `parts/basic+testing+overture+rv32i` 156/156;
`parts/sap1` 6/6; `parts/simple8` 5/5; `parts/picorv32` 13/13; `parts/sap2`
8/8 (the 7 program suites plus the new register-tile smoke).

Once Slice 1 lands, fold the `bus_driver` siblings into the register tiles
themselves so each register exposes both internal `value` (still consumed by
the ALU and instruction decode) and a real `inout wire [7:0] bus` participation
gated by `en_read`. The tile shape becomes the minimal "Ben Eater register":
write-from-bus on `en_write`, drive-onto-bus on `en_read`, value also exposed
internally.

Scope:

- Introduce `register_tile`, `register_pc_tile`, and `register_instr_tile`
  modules in [`parts/sap2/sap2.sv`](./parts/sap2/sap2.sv).
- Drop the now-redundant `bus_driver reg_a_bus` and `bus_driver pc_bus`
  instances. Keep separate `external_bus`, `alu_bus`, and `mem_bus` driver
  instances since those represent non-register sources.
- Verify the existing `parts/sap2` JSON suites still pass (the harness
  contract is unchanged).
- Add a focused `parts/sap2/sap2_register_tile.{sv,json}` smoke that exercises
  one register tile in isolation: `en_write` plus `en_read` plus the
  observable `value` output.

Exit:

- `parts/sap2` stays green at 7+ / 7+ with the tile rewrite.
- The number of `bus_driver` instances inside `machine` drops from 6 to 3.

### Slice 3: Open The Public `inout` Boundary

This is the meaningful "unlock" step. The unlock bar is satisfied; the work
left is wiring a small JSON harness syntax for external drivers and updating
runtime + validation to allow top-level `inout`.

Scope:

- Remove the top-level rejection in
  [`validate.rs::validate_supported_port_directions`](./crates/svsim/src/validate.rs)
  for `Inout`. Internal whole-net binding rules still apply for child
  instantiations.
- In `sim.rs`, add a top-level `inout` path. Treat each external `inout`
  driver as a single staged net driver per cycle whose value comes from the
  JSON harness ("released" = `LogicBit::Z`, otherwise the input value). The
  bus output observed by the harness is the resolved net value after settle.
- Extend the JSON test format with one new affordance: input values may be
  expressed as four-state strings (already supported for outputs), so a
  harness can drive `"bus": "8'bz"` to release the bus or `"bus": "8'b1010"`
  to drive a contender. Document the convention in
  [`docs/sap1-port-compromises.md`](./docs/sap1-port-compromises.md) and
  [`wiki/architecture/runtime-and-state.md`](./wiki/architecture/runtime-and-state.md).
- Add a focused `parts/testing/025-TopLevelInout.{sv,json}` that drives a
  single-bit shared bus from both the harness and an internal driver, covering
  release / drive-low / drive-high / contention cases.
- Decide whether a `parts/sap2` variant should expose its bus directly to the
  harness as `inout`. Most likely keep `parts/sap2/sap2.sv` as it is (so the
  green contract does not flip) and add a sibling
  `parts/sap2/sap2_inout_top.sv` that *does* expose a top-level `inout` bus,
  to prove the new feature on a CPU-shaped design.

Exit:

- `parts/testing/025-TopLevelInout.json` and `parts/sap2/sap2_inout_top.json`
  pass.
- The green corpus and existing auxiliary corpora remain green.
- Documentation, the wiki status page, and the corpus map note that public
  `inout` is now supported (with a brief description of the release-vs-drive
  JSON convention).

### Slice 4 (Optional): SAP-3 Sketch

A SAP-3 corpus is only worth opening if Slices 1-3 do not already feel like
"meaningfully moved." If they do not:

- Add `parts/sap3/` with a more faithful Ben Eater SAP-2 instruction set
  (additional opcodes, extended microcode width, and at minimum a 16-byte
  user-program window plus an output-port style memory-mapped device).
- Reuse the existing register-tile and `bus_driver` modules from `sap2.sv` -
  the simulator capability surface should not need to grow further for this.
- Generate the JSON program suites the same way `parts/sap2/gen_svsim.py`
  does today.

A SAP-3 slice does not require any simulator changes if Slices 1-3 land first.
That is the test for whether it is worth doing: if the only thing left is "add
a richer corpus", the simulator side of the SAP port story is complete.

## Validation Commands

After each slice, the green compatibility surface is the
[`AGENTS.md`](./AGENTS.md) one:

```text
cargo fmt --check
cargo test
cargo run -q -p svsim-cli -- \
  --json-test-dir parts/basic \
  --json-test-dir parts/testing \
  --json-test-dir parts/overture \
  --json-test-dir parts/rv32i
```

Also run the auxiliary corpora that the slice touches:

```text
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/sap1
cargo run -q -p svsim-cli -- --json-test-dir parts/sap2
cargo run -q -p svsim-cli -- --json-test-dir parts/simple8
```

For each slice that touches `parts/sap2`, also re-run `gen_svsim.py` so the
generated assets stay consistent with the SAP-1 source corpus.

## Recommended Commit Order

1. `feat(literals): lower x/z digits into LogicValue` (Slice 1, library only).
2. `corpus(sap2): drop float_bus workaround in favor of 8'bz` (Slice 1
   follow-up, source-only).
3. `corpus(sap2): fold bus_driver into register tiles` (Slice 2).
4. `feat(inout): allow top-level inout with explicit external drivers`
   (Slice 3, library + JSON harness).
5. `corpus(sap2): add inout-top sibling and focused tests` (Slice 3
   follow-up).
6. `corpus(sap3): sketch follow-on Ben Eater-style CPU` (Slice 4, optional).

Each commit should keep the green surface above at 155/155 and the auxiliary
surfaces at their current pass counts.

## What This Plan Intentionally Does Not Do

- It does not introduce delays, event controls, `initial`, or `$readmemh`.
  Those remain on the `wiki/roadmap/open-edges.md` list and stay outside this
  milestone.
- It does not add gate / switch primitives or pull devices. Drive-strength
  handling is already present in the resolver but exposing it through
  `pullup` / `tran` is not part of finishing the SAP story.
- It does not start the compiled simulation IR work. HIR-plus-runtime is
  enough to land everything above, and the IR split is a separate project.
- It does not touch `svsim-render`. Rendering remains explicitly deferred.

## Cross-References

- Active milestone backstory: [`plan.md`](./plan.md)
- Corpus inventory: [`wiki/testing/corpus-map.md`](./wiki/testing/corpus-map.md)
- Architectural snapshot:
  [`wiki/architecture/runtime-and-state.md`](./wiki/architecture/runtime-and-state.md)
- Open simulator gaps:
  [`wiki/roadmap/open-edges.md`](./wiki/roadmap/open-edges.md)
- Original SAP-1 import friction:
  [`docs/sap1-port-compromises.md`](./docs/sap1-port-compromises.md)
- Current SAP-2 source:
  [`parts/sap2/sap2.sv`](./parts/sap2/sap2.sv) and
  [`parts/sap2/sap2_bus_semantics.sv`](./parts/sap2/sap2_bus_semantics.sv)
