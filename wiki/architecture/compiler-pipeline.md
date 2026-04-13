# Compiler Pipeline

The runtime does not execute `sv-parser` syntax trees directly. The project compiles source files into an owned representation first, validates that representation, elaborates the chosen top, and only then creates runtime state.

## End-To-End Flow

1. `Compiler` accepts a file path, virtual source string, compile directory, or JSON test directory.
2. `SvParserFrontend` parses the source and lowers only the supported subset into owned `SourceFile` and `HirDesign` values.
3. `HirDesign` stores modules, ports, parameters, nets, variables, memories, instantiations, continuous assignments, procedural blocks, statements, expressions, and lvalues in parser-independent Rust types.
4. Validation walks that owned design and rejects unsupported or inconsistent constructs before runtime. This includes duplicate declarations, bad instance bindings, missing modules, public/top-level `inout`, unsupported `ref`, non-lvalue output or `inout` bindings, and invalid memory/index/value-shape cases.
5. `CompiledDesign` packages the lowered HIR, top-module choice, and search paths, then exposes `hierarchy()`, `elaborate()`, `instantiate_top()`, and `run_json_file()`.
6. `ElaboratedDesign` expands the chosen top into a typed instance tree with explicit ports, nets, variables, memories, and instance bindings.
7. `SimulationSession` executes the elaborated design through either combinational settle or cycle-stepped sequential simulation.
8. The CLI and JSON harness sit on top of the same library entry points rather than reimplementing simulator logic.

## Supported Frontend And HIR Surface

- Modules, ANSI ports, declarations, parameters, continuous assignments, and named-port instantiations.
- `always_comb`, Verilog-2001 `always @*`, `always_ff @(posedge ...)`, and `always @(posedge ...)` lowered to the supported forms.
- Blocking and nonblocking procedural assignment.
- `if` / `else`, `case` / `default`, and statement blocks.
- Fixed-size single-dimension memories with reads and single-element procedural writes.
- Concatenation, replication, bit selects, part selects, arithmetic, bitwise, logical, equality, ternary, and cast-style signedness expressions, now evaluated with four-state-aware runtime semantics.

## Important Validation Boundaries

- Public/top-level `inout` is still rejected.
- Internal `inout` is currently only allowed when an instantiated child port is bound to a whole parent net signal.
- `ref` ports are still rejected.
- Recursive instantiation is rejected.
- Unknown named ports, duplicate named-port connections, missing child input bindings, and non-lvalue output bindings are rejected.
- `inout` connections to part selects, memory elements, or arbitrary expressions are rejected.
- Legacy `rom_*` wrappers are validated instead of being treated as a magical runtime side path.
- Width validation is current code behavior, not the older "u64 only" story from early plans. The current `BitValue` implementation is limb-backed, and the runtime no longer hard-caps widths at `64`; zero-width shapes are still rejected.

## Still Missing In The Architecture

- A compiled simulation IR distinct from HIR.
- Broader public `inout` support beyond the current internal whole-net subset.
- Delay-based or general event-control simulation.
- Render integration on top of stable structured artifacts.

## Sources

- [../../crates/svsim/src/compiler.rs](../../crates/svsim/src/compiler.rs)
- [../../crates/svsim/src/frontend/sv_parser.rs](../../crates/svsim/src/frontend/sv_parser.rs)
- [../../crates/svsim/src/hir.rs](../../crates/svsim/src/hir.rs)
- [../../crates/svsim/src/elaborate.rs](../../crates/svsim/src/elaborate.rs)
- [../../crates/svsim/src/validate.rs](../../crates/svsim/src/validate.rs)
- [../../crates/svsim/src/design.rs](../../crates/svsim/src/design.rs)
- [../../plan.md](../../plan.md)
- [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md)
