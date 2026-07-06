# Overview

`svsim` is a Rust rewrite of a Python SystemVerilog simulator (the reference implementation is no longer vendored in this checkout; the `parts/` corpus is the behavior oracle). The rewrite is intentionally library-first: the main product is an embeddable Rust crate, while the CLI is a thin wrapper that exposes parse, compile, and JSON-regression flows.

## What The Project Optimizes For

- A clearly bounded executable subset of SystemVerilog instead of a vague partial implementation.
- Parity with the checked-in `parts/` corpus before broadening language support.
- A clean handoff from parser-specific syntax into owned HIR.
- An explicit elaboration and runtime layer that can grow toward richer net semantics without dragging `sv-parser` types through the system.
- Explicit memory and program binding APIs, with only narrow compatibility fallbacks for legacy corpus conventions.

## Mental Model

- `sv-parser` is the frontend, not the runtime model.
- `crates/svsim/src/hir.rs` is the owned intermediate layer the rest of the simulator works against.
- `ElaboratedDesign` is the structural bridge between HIR and runtime state.
- Validation happens before simulation starts and is part of the product boundary, not an afterthought.
- `CompiledDesign` is the boundary between compilation and execution.
- `LogicValue` is now the primary HDL-facing runtime value type; `BitValue` remains the host-facing 2-state convenience type.
- `SimulationSession` supports both combinational settle (`eval_once`) and cycle-stepped sequential execution (`step`).
- JSON suites are first-class and act as the main executable compatibility harness.

## What It Is Not

- Not a full IEEE 1800 simulator.
- Not render-first. Structured compile and test artifacts come first; rendering is intentionally deferred.
- Not a direct architectural copy of the Python reference.

## Good Starting Paths

- For parser or lowering work: [architecture/compiler-pipeline.md](./architecture/compiler-pipeline.md)
- For execution or state bugs: [architecture/runtime-and-state.md](./architecture/runtime-and-state.md)
- For choosing regression targets: [testing/corpus-map.md](./testing/corpus-map.md)
- For the completed four-state / `inout` / `sap2` milestone: [roadmap/inout-and-sap2-milestone.md](./roadmap/inout-and-sap2-milestone.md)
- For the active architectural review campaign: [../plans/in-progress/architectural-review.md](../plans/in-progress/architectural-review.md)
- For the latest verified status: [status/current-state.md](./status/current-state.md)

## Sources

- [../AGENTS.md](../AGENTS.md)
- [../plans/completed/plan-sap2-inout.md](../plans/completed/plan-sap2-inout.md)
- [../docs/rust-port-plan.md](../docs/rust-port-plan.md)
- [../Cargo.toml](../Cargo.toml)
- [../crates/svsim/src/lib.rs](../crates/svsim/src/lib.rs)
- [../crates/svsim-cli/src/main.rs](../crates/svsim-cli/src/main.rs)
- [../crates/svsim-render/src/lib.rs](../crates/svsim-render/src/lib.rs)
