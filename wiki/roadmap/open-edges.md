# Open Edges

The project is healthy, but it is not "done." The most important gaps are no longer basic parser bring-up; they are the places where imported designs, richer runtime semantics, or future product layers still need work.

## Near-Term Bounded Targets

- ~~Decide when, or whether, to remove the intentional public/top-level `inout` rejection now that the internal runtime slice and `sap2` auxiliary corpus are working.~~ (resolved 2026-04-16; public top-level `inout` is now supported, see [`../../plan-next.md`](../../plan-next.md) Slice 3.)
- ~~Extend the `sap2` shared-bus structure beyond the current leaf-driver slice if a closer import of the original machine partitioning is worth the added complexity.~~ (resolved 2026-04-16; register tiles fold the bus drivers into the register modules, see [`../../plan-next.md`](../../plan-next.md) Slice 2.)
- ~~Decide whether HDL literal lowering should carry explicit `x` / `z` digits natively instead of leaning on floated nets for some internal-bus patterns.~~ (resolved 2026-04-16; native four-state literals landed with [`../../plan-next.md`](../../plan-next.md) Slice 1.)

## Language And Runtime Gaps

- Internal `inout` support is still narrow: only whole parent net bindings are accepted today.
- `ref` ports remain unsupported.
- Gate/switch primitives and pull devices (`tran`, `bufif`, `pullup`, and friends) are still deferred.
- Testbench-oriented features such as `initial`, delay-based flows, and broader event controls are still out of scope.
- Standard memory initialization idioms such as `$readmemh` are still a gap compared with mainstream simulator flows.
- Ordered port connections and some declaration-initializer patterns still need normalization or are not yet supported in the main path.

## Structural Gaps

- Elaboration now exists, but there is still no compiled simulation IR beyond HIR plus runtime structures.
- HIR is still closer to the execution path than the eventual compiled IR vision.
- `svsim-render` has not been turned into a real rendering layer yet.

## Portability Lessons

- SAP-1 shows the main pain points for imported designs: shared buses, source-level memory init, and harness-free testbench flows.
- SAP-2 shows which of those pain points have started to move: internal shared buses and contention/floating behavior now have a first-class auxiliary corpus.
- PicoRV32 is a reminder that "compile-green" and "runtime-covered" are different levels of confidence.
- The best next features are the ones that reduce the amount of design-specific scaffolding needed to import existing Verilog cleanly.

## Sources

- [../../plan.md](../../plan.md)
- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md)
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md)
- [../../docs/sap1-port-compromises.md](../../docs/sap1-port-compromises.md)
- [../../parts/picorv32/README.md](../../parts/picorv32/README.md)
- [../../parts/sap2/README.md](../../parts/sap2/README.md)
