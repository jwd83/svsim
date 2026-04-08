# Open Edges

The project is healthy, but it is not "done." The most important gaps are no longer basic parser bring-up; they are the places where imported designs, richer runtime semantics, or future product layers still need work.

## Near-Term Bounded Targets

- Finish the remaining PicoRV32 branch-side instruction-path trap case described in the latest checked-in progress notes.
- Keep compile-only and executable coverage clearly separated in the docs and in future reports.
- Continue shifting hot-path logic away from repeated runtime checks and toward more explicit compiled evaluation structure.

## Language And Runtime Gaps

- `inout` and `ref` ports remain unsupported.
- Testbench-oriented features such as `initial`, delay-based flows, and broader event controls are still out of scope.
- Standard memory initialization idioms such as `$readmemh` are still a gap compared with mainstream simulator flows.
- Ordered port connections and some declaration-initializer patterns still need normalization or are not yet supported in the main path.

## Structural Gaps

- Elaboration is still lighter-weight than the long-term architecture sketch.
- HIR is still closer to the execution path than the eventual compiled IR vision.
- `svsim-render` has not been turned into a real rendering layer yet.

## Portability Lessons

- SAP-1 shows the main pain points for imported designs: shared buses, source-level memory init, and harness-free testbench flows.
- PicoRV32 is a reminder that "compile-green" and "runtime-covered" are different levels of confidence.
- The best next features are the ones that reduce the amount of design-specific scaffolding needed to import existing Verilog cleanly.

## Sources

- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md)
- [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md)
- [../../docs/sap1-port-compromises.md](../../docs/sap1-port-compromises.md)
- [../../parts/picorv32/README.md](../../parts/picorv32/README.md)
