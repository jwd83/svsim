# Workspace Map

The workspace is intentionally small. Most of the simulator lives in one core crate, with a thin CLI wrapper and a placeholder render crate beside it.

## Top-Level Layout

| Path | Role |
| --- | --- |
| [`../../crates/svsim`](../../crates/svsim) | Core library crate. Parsing, HIR, validation, design handles, simulation, and JSON harness execution live here. |
| [`../../crates/svsim-cli`](../../crates/svsim-cli) | Thin CLI wrapper over the library APIs. |
| [`../../crates/svsim-render`](../../crates/svsim-render) | Placeholder render crate. Current status is literally `"rendering deferred"`. |
| [`../../parts`](../../parts) | Compatibility corpus, demo designs, harness assets, and negative tests. Green dirs are gated by [`corpus_gate.rs`](../../crates/svsim/tests/corpus_gate.rs). |
| [`../../docs`](../../docs) | Progress reports, port-specific design notes, and generated corpus reports (`docs/tests/`). |
| [`../../plans`](../../plans) | Plan lifecycle: `in-progress/` and `completed/`, including the completed 2026-07-06 architectural review. |
| [`../../wiki`](../../wiki) | Persistent synthesized knowledge layer that summarizes and cross-links the code and corpus. |

## Main `svsim` Modules

| Module | Responsibility |
| --- | --- |
| [`../../crates/svsim/src/compiler.rs`](../../crates/svsim/src/compiler.rs) | Entry points for file/string compilation plus compile-dir and JSON-dir batch runs. |
| [`../../crates/svsim/src/frontend`](../../crates/svsim/src/frontend) | `sv-parser` integration and lowering into owned HIR. Split by responsibility under [`sv_parser/`](../../crates/svsim/src/frontend/sv_parser/): `module_structure.rs`, `statements.rs`, `expressions.rs`, `literals.rs`, `const_eval.rs` (lowering-time constant folding + frozen-parameter recording), and `loop_unroll.rs` (for-loop unrolling — elaboration work done at lowering time). |
| [`../../crates/svsim/src/hir.rs`](../../crates/svsim/src/hir.rs) | The parser-independent executable model: modules, ports, memories, statements, expressions, and lvalues. |
| [`../../crates/svsim/src/validate.rs`](../../crates/svsim/src/validate.rs) | Compile-time semantic checks, including whole-net `inout` binding rules. |
| [`../../crates/svsim/src/design.rs`](../../crates/svsim/src/design.rs) | `CompiledDesign`, hierarchy reporting, elaboration access, and runtime instantiation entry points. |
| [`../../crates/svsim/src/elaborate.rs`](../../crates/svsim/src/elaborate.rs) | Typed elaboration layer that turns HIR modules into runtime object shapes and instance bindings. |
| [`../../crates/svsim/src/logic_value.rs`](../../crates/svsim/src/logic_value.rs) | Four-state runtime values, parsing, formatting, and wildcard expectation matching. |
| [`../../crates/svsim/src/logic_ops.rs`](../../crates/svsim/src/logic_ops.rs) | Crate-private four-state primitive operations (bit truth tables, reductions, slices, sign extension) with direct truth-table unit tests. |
| [`../../crates/svsim/src/expr_eval.rs`](../../crates/svsim/src/expr_eval.rs) | The single shared HIR expression evaluator (`Value`, combinators, `eval_expr`, parameter resolution) used by the runtime, validation, frontend constant folding, and elaboration. |
| [`../../crates/svsim/src/net_resolve.rs`](../../crates/svsim/src/net_resolve.rs) | Zero-delay per-bit net resolution and drive-strength handling. |
| [`../../crates/svsim/src/sim/`](../../crates/svsim/src/sim/) | Structural runtime, split by responsibility: `session.rs` (public API + settle/step scheduler), `eval.rs` (expression/lvalue evaluation, driver staging), `state.rs` (hierarchical module state, bindings), `memory.rs` (memory files), `legacy_rom.rs` (documented `rom_*` compatibility shim), `value.rs` (runtime object values and public-bits boundary). |
| [`../../crates/svsim/src/test.rs`](../../crates/svsim/src/test.rs) | JSON suite parsing, execution, tracing, and report types. |
| [`../../crates/svsim/src/diag.rs`](../../crates/svsim/src/diag.rs) | Diagnostics and top-level error types. |
| [`../../crates/svsim/src/width.rs`](../../crates/svsim/src/width.rs) | Width inference plus shared shift/sign-extension helpers. |
| [`../../crates/svsim/src/bit_value.rs`](../../crates/svsim/src/bit_value.rs) | Limb-backed bit-vector type used by validation and simulation. |
| [`../../crates/svsim/tests/corpus_gate.rs`](../../crates/svsim/tests/corpus_gate.rs) | Green-corpus gate: one test per green `parts/` directory; `cargo test` fails if any regression suite fails. |

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
