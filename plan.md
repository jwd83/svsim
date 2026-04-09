# Inout / SAP2 Next-Turn Plan

## Goal

Add full zero-delay Verilog net semantics, real `inout` support, and a new
`parts/sap2` corpus without destabilizing the existing green compatibility
surface.

This plan is intentionally infrastructure-first. `sap2` is the integration
target at the end of each major phase, but it starts as a non-gating auxiliary
corpus. Public `inout` stays rejected until the full semantic slice is ready.

## Locked Decisions

- Keep `BitValue` as the numeric/host-facing 2-state type.
- Add a new HDL runtime value layer (`LogicBits` / `LogicValue`) for four-state
  semantics.
- Introduce an explicit elaboration layer between HIR and the runtime.
- Make ports explicitly typed by direction and storage kind.
- Model nets as first-class runtime objects with per-bit resolution.
- Support the full net-kind / drive-strength matrix, but keep the milestone at
  zero-delay semantics.
- Use standard Verilog four-state expression/control semantics, with no custom
  xprop mode in this milestone.
- Make the public boundary four-state-aware, but add explicit 2-state
  convenience wrappers.
- Evolve JSON in place:
  - plain numeric values remain valid 2-state shorthand
  - four-state values use explicit string forms
  - `?` is the wildcard token
  - literal `x` / `z` remain meaningful values, not wildcards
- Keep public `inout` rejected until the unlock bar below is fully met.
- Add a parallel `parts/sap2` corpus that keeps the same harness-visible top
  contract as `sap1`, while making the internal machine structure more original.
- Reuse the `sap1` assembler examples, RAM images, and microcode for `sap2`,
  then add focused `sap2`-specific tests for floating and contention behavior.
- Defer switch/gate primitives and pull devices (`tran`, `bufif`, `pullup`,
  etc.) to later work.

## Public `inout` Unlock Bar

Do not remove the public `inout` rejection until all of these are true:

1. The elaboration layer owns typed ports, net kinds, storage kinds, and
   instance bindings.
2. The runtime has per-bit net resolution and shared structural connectivity.
3. Four-state expression and control semantics are implemented.
4. The public API, CLI, JSON tests, traces, and reports can round-trip literal
   `x` / `z` values and wildcard `?` expectations.
5. The existing green corpora still pass.
6. `parts/sap2` and focused `inout` / floating / contention tests pass.

## Phases

### Phase 1: Four-State Value And Harness Format

Scope:

- Add `LogicBits` / `LogicValue` and conversion helpers to/from `BitValue`.
- Define serde / display / parse rules for four-state values.
- Extend JSON test parsing and reporting to support explicit four-state values.
- Add wildcard-aware expectation matching using `?`.
- Keep the current simulator behavior and public `inout` rejection unchanged.

Exit:

- New unit tests cover parsing, serialization, wildcard matching, and 2-state
  fallback behavior.
- Existing JSON corpora continue to work without fixture changes.

### Phase 2: Typed HIR And Elaboration Scaffolding

Scope:

- Enrich HIR declarations and ports with storage kind and net-kind metadata.
- Preserve net-vs-variable distinctions from the frontend instead of collapsing
  everything into `SignalDecl`.
- Add an explicit elaboration layer that computes instance bindings and runtime
  object shapes without yet replacing the full simulator.
- Check in a `parts/sap2` skeleton:
  - `README.md`
  - copied assembler/example assets
  - adapted generation script
  - placeholder source file(s)

Exit:

- Elaboration can represent typed ports, nets, variables, and memories.
- `parts/sap2` exists as a non-gating corpus skeleton.

### Phase 3: Structural Runtime Cutover

Scope:

- Replace the current copy-based cross-instance data flow with structurally
  elaborated shared runtime objects.
- Introduce first-class runtime net nodes, variable cells, and driver endpoints.
- Keep 2-state behavior for existing designs where possible.
- Do not enable public `inout` yet.

Exit:

- Existing green corpora still pass through the new elaborated runtime path.
- Focused structural elaboration tests cover shared connectivity and instance
  binding.

### Phase 4: Net Resolution Engine

Scope:

- Implement per-bit resolution.
- Add net-kind behavior and drive-strength handling.
- Support `Z` and contention-driven `X` results at the net layer.
- Add focused tests for:
  - floating nets
  - conflicting strong drives
  - non-conflicting multi-driver cases
  - representative net-kind / strength combinations

Exit:

- The runtime can faithfully resolve four-state net behavior.
- Public `inout` remains rejected until expression/control semantics land too.

### Phase 5: Four-State Expressions And Control

Scope:

- Upgrade expression evaluation from 2-state-style truthiness/equality rules to
  standard Verilog four-state behavior.
- Cover unary, bitwise, logical, equality, comparison, ternary, `if`, and
  `case` behavior in focused tests.
- Keep the milestone zero-delay only.

Exit:

- Four-state values propagate correctly beyond the net layer.
- The simulator no longer silently collapses `X` / `Z` in control decisions.

### Phase 6: Public Boundary Flip

Scope:

- Make the primary public runtime APIs four-state-aware.
- Add explicit 2-state convenience wrappers that fail on `x` / `z`.
- Update CLI and JSON report surfaces to emit four-state values.
- Remove the public `inout` rejection only if the unlock bar is satisfied.

Exit:

- Users can compile and run real `inout` designs with correct public-facing
  semantics.
- Existing callers still have a clear 2-state wrapper path.

### Phase 7: SAP2 Integration

Scope:

- Implement `parts/sap2` as a parallel corpus:
  - harness-facing wrapper keeps the current `sap1`-style top contract
  - internal core moves closer to the original shared-bus structure
  - leaf bus participants use real `inout` with internal high-impedance drive
- Reuse the existing SAP program corpus and microcode to generate parallel
  `sap2` JSON suites.
- Add focused `sap2`-specific bus tests for floating and contention behavior.

Exit:

- `parts/sap2` program suites match the intended harness-visible behavior.
- Focused bus-semantics tests cover the new structural behavior explicitly.

## Recommended Commit / PR Slices

1. Add `LogicValue` types, parsing, formatting, and conversion helpers.
2. Extend JSON expectations, reports, and wildcard matching for four-state
   values.
3. Enrich HIR/frontend typing for ports, nets, and variables.
4. Add elaboration layer types and tests, but keep the current runtime active.
5. Cut the runtime over to elaborated structural connectivity.
6. Add the per-bit net-resolution engine and focused resolver tests.
7. Upgrade expression/control semantics to four-state behavior.
8. Flip the public API/CLI/JSON boundary to four-state and add 2-state wrappers.
9. Remove public `inout` rejection once the unlock bar is met.
10. Add `parts/sap2`, the reused program corpus, and focused bus tests.

Each slice should be reviewable on its own and should preserve the existing
green surface as much as possible.

## Validation Commands

At minimum after each major slice:

- `cargo test -p svsim`
- `cargo test`
- `cargo run -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i`

Additional non-gating checks when available:

- `cargo run -p svsim-cli -- --json-test-dir parts/picorv32`
- `cargo run -p svsim-cli -- --json-test-dir parts/sap2`

## Next Turn

Use this document as the starting point for the next turn:

1. stress-test the phase ordering
2. identify the riskiest slice boundaries
3. decide the first concrete implementation slice to actually land
