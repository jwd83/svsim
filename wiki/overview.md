# Overview

`svsim` is a Rust rewrite of the SystemVerilog simulator living in [`ref/`](../ref/). The rewrite is intentionally library-first: the main product is an embeddable Rust crate, while the CLI is a thin wrapper that exposes parse, compile, and JSON-regression flows.

## What The Project Optimizes For

- A clearly bounded executable subset of SystemVerilog instead of a vague partial implementation.
- Parity with the checked-in `parts/` corpus before broadening language support.
- A clean handoff from parser-specific syntax into owned HIR.
- Explicit memory and program binding APIs, with only narrow compatibility fallbacks for legacy corpus conventions.

## Mental Model

- `sv-parser` is the frontend, not the runtime model.
- `crates/svsim/src/hir.rs` is the owned intermediate layer the rest of the simulator works against.
- Validation happens before simulation starts and is part of the product boundary, not an afterthought.
- `CompiledDesign` is the boundary between compilation and execution.
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
- For the latest verified status: [status/current-state.md](./status/current-state.md)

## Sources

- [../AGENTS.md](../AGENTS.md)
- [../docs/rust-port-plan.md](../docs/rust-port-plan.md)
- [../Cargo.toml](../Cargo.toml)
- [../crates/svsim/src/lib.rs](../crates/svsim/src/lib.rs)
- [../crates/svsim-cli/src/main.rs](../crates/svsim-cli/src/main.rs)
- [../crates/svsim-render/src/lib.rs](../crates/svsim-render/src/lib.rs)
