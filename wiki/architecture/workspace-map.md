# Workspace Map

The workspace is intentionally small. Most of the simulator lives in one core crate, with a thin CLI wrapper and a placeholder render crate beside it.

## Top-Level Layout

| Path | Role |
| --- | --- |
| [`../../crates/svsim`](../../crates/svsim) | Core library crate. Parsing, HIR, validation, design handles, simulation, and JSON harness execution live here. |
| [`../../crates/svsim-cli`](../../crates/svsim-cli) | Thin CLI wrapper over the library APIs. |
| [`../../crates/svsim-render`](../../crates/svsim-render) | Placeholder render crate. Current status is literally `"rendering deferred"`. |
| [`../../parts`](../../parts) | Compatibility corpus, demo designs, harness assets, and negative tests. |
| [`../../docs`](../../docs) | Plans, progress reports, and port-specific design notes. |
| [`../../ref`](../../ref) | Python reference implementation and historical planning context. |

## Main `svsim` Modules

| Module | Responsibility |
| --- | --- |
| [`../../crates/svsim/src/compiler.rs`](../../crates/svsim/src/compiler.rs) | Entry points for file/string compilation plus compile-dir and JSON-dir batch runs. |
| [`../../crates/svsim/src/frontend`](../../crates/svsim/src/frontend) | `sv-parser` integration and lowering into owned HIR. |
| [`../../crates/svsim/src/hir.rs`](../../crates/svsim/src/hir.rs) | The parser-independent executable model: modules, ports, memories, statements, expressions, and lvalues. |
| [`../../crates/svsim/src/validate.rs`](../../crates/svsim/src/validate.rs) | Compile-time semantic checks and legacy ROM compatibility validation. |
| [`../../crates/svsim/src/design.rs`](../../crates/svsim/src/design.rs) | `CompiledDesign`, hierarchy reporting, and runtime instantiation entry points. |
| [`../../crates/svsim/src/sim.rs`](../../crates/svsim/src/sim.rs) | Combinational settle, sequential stepping, memory loading, and signal inspection. |
| [`../../crates/svsim/src/test.rs`](../../crates/svsim/src/test.rs) | JSON suite parsing, execution, tracing, and report types. |
| [`../../crates/svsim/src/diag.rs`](../../crates/svsim/src/diag.rs) | Diagnostics and top-level error types. |
| [`../../crates/svsim/src/width.rs`](../../crates/svsim/src/width.rs) | Width inference plus shared shift/sign-extension helpers. |
| [`../../crates/svsim/src/bit_value.rs`](../../crates/svsim/src/bit_value.rs) | Limb-backed bit-vector type used by validation and simulation. |

## Practical Reading Order

1. Start with [../../crates/svsim/src/lib.rs](../../crates/svsim/src/lib.rs) for the public surface.
2. Read [compiler-pipeline.md](./compiler-pipeline.md) for the end-to-end flow.
3. Read [runtime-and-state.md](./runtime-and-state.md) if the bug is about behavior instead of compilation.

## Sources

- [../../AGENTS.md](../../AGENTS.md)
- [../../Cargo.toml](../../Cargo.toml)
- [../../crates/svsim/src/lib.rs](../../crates/svsim/src/lib.rs)
- [../../crates/svsim-cli/src/main.rs](../../crates/svsim-cli/src/main.rs)
- [../../crates/svsim-render/src/lib.rs](../../crates/svsim-render/src/lib.rs)
