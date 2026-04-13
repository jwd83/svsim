# Four-State, Inout, And SAP-2 Milestone

This page answers one question: what has actually landed from the current zero-delay net-semantics milestone, and what is still intentionally held back?

## Landed And Verified On 2026-04-12

- `LogicBits` / `LogicValue` exist and are now the main HDL-facing runtime value layer.
- JSON tests, traces, CLI output, and the primary runtime APIs round-trip literal four-state values.
- `ElaboratedDesign` is live, so runtime state is built from typed ports, nets, variables, memories, and instance bindings instead of the older copy-heavy path.
- Zero-delay per-bit net resolution is live, including floating `z` and contention-driven `x`.
- Expression and control semantics preserve `x` / `z` instead of silently collapsing them.
- Internal whole-net `inout` leaf ports are allowed when they bind to parent nets.
- `parts/sap2` is runnable and green as an auxiliary corpus, including both long-running program suites and a focused shared-bus smoke test.

## Still Intentionally Locked

- Public/top-level `inout` remains rejected.
- Internal `inout` is still a bounded feature: only whole parent net bindings are supported, not part selects, memories, or arbitrary expressions.
- The milestone is still zero-delay only; switch primitives, pull devices, and broader event/timing semantics remain deferred.

## Why SAP-2 Matters

- `sap1` documented a major product gap: imported shared-bus designs had to be rewritten into explicit mux structures.
- `sap2` is the first checked-in evidence that the simulator can reverse part of that compromise while preserving the same harness-visible top contract.
- The focused `sap2_bus_semantics` suite turns floating and contention behavior into something regression-tested rather than merely described in planning notes.

## Best Next Questions

- Is the current leaf-only shared-bus slice enough, or should more of the SAP-2 machine move onto the `inout` fabric?
- Should HDL literal lowering learn to carry explicit `x` / `z` digits natively instead of relying on floated nets in some internal-bus patterns?
- At what point does keeping public/top-level `inout` rejected become more confusing than helpful?

## Sources

- [../../plan.md](../../plan.md)
- [../../parts/sap2/README.md](../../parts/sap2/README.md)
- [../../parts/sap2/sap2.sv](../../parts/sap2/sap2.sv)
- [../../parts/sap2/sap2_bus_semantics.sv](../../parts/sap2/sap2_bus_semantics.sv)
- [../../crates/svsim/src/logic_value.rs](../../crates/svsim/src/logic_value.rs)
- [../../crates/svsim/src/elaborate.rs](../../crates/svsim/src/elaborate.rs)
- [../../crates/svsim/src/sim.rs](../../crates/svsim/src/sim.rs)
- [../../crates/svsim/src/validate.rs](../../crates/svsim/src/validate.rs)
