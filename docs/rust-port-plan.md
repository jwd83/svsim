# Rust Port Plan

## Goal

Build a Rust SystemVerilog simulator that is easier to embed, faster than the Python reference, and cleanly structured enough to grow from the current educational subset into larger CPU-oriented designs.

The primary product should be a library crate. A CLI should exist, but only as a thin wrapper over the library API.

## Decisions Confirmed

These product decisions are fixed for the first Rust pass:

- target parity with the current `parts/` corpus before broadening SystemVerilog support
- make the embeddable surface Rust-only for now
- treat the CLI as the cross-language integration point
- defer PNG generation until after the simulator can emit stable structured results
- keep the library memory/program API explicit, while allowing narrowly scoped corpus-compatibility fallbacks for legacy `rom_*` and `pgm_*` fixtures in regression execution

## Current Status

Current implementation status as of March 17, 2026:

- Cargo workspace created with `svsim`, `svsim-cli`, and `svsim-render`
- `sv-parser` integrated for file-based parsing and in-memory source parsing via virtual paths
- owned HIR covers source files, module summaries, continuous assignments, instantiations, `always_comb`, a first `always_ff @(posedge ...)` subset, concatenation/replication expressions, and concatenated assignment targets
- library API now exposes `Compiler`, `CompiledDesign`, and `SimulationSession`, including `compile_file` and `compile_str`
- compilation now rejects duplicate module definitions during source registration and includes a semantic validation pass over lowered HIR, catching duplicate declarations, duplicate instance names, undeclared identifiers/memories, lowered select bounds, constant out-of-range memory indices, unsupported `inout` / `ref` ports, invalid named-port instance bindings including missing child input bindings, attempts to drive input ports, malformed legacy `rom_*` wrappers, missing legacy ROM backing files, and declarations or lowered value-shape constructs that fall outside the current supported `1..=64` bit runtime subset before simulation
- `CompiledDesign::hierarchy()` now exposes an owned top-down instance tree so embedding callers can discover valid instance paths before using per-instance memory APIs
- compile-only corpus reporting is now available through `Compiler::run_compile_dir` / `Compiler::run_compile_dirs`, and compile-only runs fail cleanly when source compilation errors or unsupported-feature diagnostics are present
- library-side JSON regression execution is now available through `CompiledDesign::run_json_file` / `svsim::JsonTestSuite` for combinational arrays, sequential `test_cases`, and relative memory-file preload
- `Compiler::run_json_test_dir`, `Compiler::run_json_test_dirs`, and repeated CLI `--json-test-dir` flags now cover both per-directory and multi-directory corpus regressions, execute suites in parallel within each directory, emit deterministic structured reports, and can discover JSON-only suites that explicitly declare a shared source file
- CLI can parse a SystemVerilog file and emit JSON describing discovered modules, or run compile-only and JSON batch regressions through repeated `--compile-dir` and `--json-test-dir` flags plus single-suite `--json-test`
- `SimulationSession::eval_once` can execute hierarchical combinational designs with fixed-point convergence across continuous assignments, module instances, and a basic `always_comb` subset, and now memoizes child instance outputs within each settle pass when their input maps remain unchanged
- `always_comb` execution now compares each block's final post-statement state against the prior iteration, so blocks that assign the same signal multiple times per execution settle correctly instead of oscillating
- runtime width handling now includes a shared lowered-expression width helper, fixed-width ternary evaluation, and an explicit coercion path for assignments and instance-port handoff, so self-determined `?:` results and in-range truncation/zero-extension no longer depend on scattered incidental `u64` shaping
- `SimulationSession::step` now maintains per-instance state and can advance hierarchical designs using `always_ff @(posedge <clock>)` blocks with blocking immediate updates and nonblocking assignment staging
- current procedural subset: blocking assignments in combinational blocks and `always_ff`, nonblocking assignments in `always_ff`, `if` / `else`, `case` / `default`, and `begin` / `end` statement blocks
- current expression subset adds concatenation, replication, logical `&&` / `||`, equality `==` / `!=`, arithmetic `+` / `-`, logical shifts `<<` / `>>`, and single-dimension memory reads on top of literals, identifiers, slices, ternary expressions, and bitwise operators
- frontend lowering now accepts grouped ANSI port declarations such as `input [4:0] a, b, c` and lowers net declaration initializers like `wire [24:0] v = expr;` into signal declarations plus continuous assignments
- `compile_str` can seed a design from an in-memory top module while still resolving instantiated dependencies from the virtual path's directory and configured search paths
- current sequential limits: only `posedge` event controls are lowered, `always_ff` clock expressions must be local identifiers, and cross-block race semantics are not modeled beyond deterministic source order
- current memory subset supports fixed-size unpacked `reg` / `logic` arrays with zero-initialized reads, explicit programmatic preload/read access by instance path, text-file ROM/RAM loading, procedural single-element writes, and JSON-driven regression preload; explicit elaboration, broader event controls, and render integration are still pending
- regression compatibility now covers the legacy corpus conventions that still matter in `parts/`: compile-time validation now enforces the supported interface-only `rom_*` wrapper shape and backing text-file lookup, and `pgm_*` JSON suites auto-bind `overture_fetch.rom` from a sibling program text file when no explicit memory bindings are present
- measured verification: `cargo test` passes; the compile-only multi-directory corpus reports `parts/basic` at `44/44`, `parts/testing` at `42/42`, `parts/overture` at `41/41`, `parts/rv32i` at `1/1`, and the full `parts/basic` + `parts/testing` + `parts/overture` + `parts/rv32i` compile surface at `128/128` in about `1.6s`; the JSON regression corpus remains green at `142/142` in about `17.7s`
- measured batch status: the compile-only multi-directory runner completes `parts/basic` in about `0.4s`, `parts/testing` in about `0.1s`, `parts/overture` in about `0.6s`, and `parts/rv32i` in about `0.4s`; the JSON multi-directory runner completes them in about `7.1s`, `0.2s`, `9.1s`, and `1.4s`
- `parts/testing/020-WidthCoercion.sv` and `parts/testing/020-WidthCoercion.json` now pin widened and narrowed assignment/instance-port coercion behavior in the green corpus
- `parts/testing/021-ShiftOps.sv` and `parts/testing/021-ShiftOps.json` now pin logical shift semantics, including left-operand result width and zeroing when the shift amount reaches the operand width
- `parts/rv32i` now provides a 13-suite RV32I demo corpus covering arithmetic, compare, branch, jump, instruction-address misalignment traps, subword memory, fence/system instructions, breakpoint and illegal-instruction traps, and misaligned load/store traps
- `parts/rv32i/demo_shift_ops.json` now exercises the RV32I demo core's `SLLI`, `SRLI`, `SRAI`, `SLL`, `SRL`, and `SRA` execution path, including register-form `shamt[4:0]` masking
- `parts/testing/019-Vector5.json` now matches the standard SystemVerilog bit ordering for multi-expression replication (`{5{a, b, c, d, e}}`), retiring the old Python reference divergence from the checked-in corpus

## Compatibility Target

The first meaningful milestone is feature parity with the subset exercised by the current repository:

- `128` compile-green SystemVerilog files across `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i`
- hierarchical modules built from gates up through the Overture CPU
- combinational logic with buses, slices, concatenation, replication, arithmetic, comparisons, and ternary expressions
- `always_comb`
- `always_ff @(posedge clk)` with blocking and nonblocking assignment
- ROM and RAM style memory arrays
- JSON-backed combinational and sequential tests
- structured compile-only and JSON corpus reports

This is not a full IEEE 1800 simulator. The Rust version should intentionally support a well-defined executable subset and emit good diagnostics for unsupported constructs.

## Problems In The Python Reference

The reference project in `ref/` works, but it has a few design constraints that should not carry into the Rust rewrite:

- parsing is regex-heavy and tightly coupled to execution
- the parser, elaboration, evaluator, renderer, CLI, and test runner all live in one file
- combinational and sequential execution share logic informally instead of through a compiled IR
- module loading and caching are global, ad hoc, and not ideal for embedding
- image generation is part of the main simulator path instead of an optional layer
- the API boundary is effectively the CLI, not a reusable library surface

## Recommended Rust Stack

Use standard crates where possible instead of building custom infrastructure:

- Parsing / syntax: `sv-parser`
  - Use it as the front end for tokenization, preprocessing, and syntax parsing.
  - Do not use its parse tree as the execution model. Lower once into our own owned HIR.
- Future image output: `image`
  - Use it for image buffers and PNG encoding.
  - Add `imageproc` plus `ab_glyph` only for drawing helpers and text.
- JSON test I/O: `serde` and `serde_json`
- Parallel batch testing: `rayon`

## High-Level Architecture

Keep the initial workspace small. Avoid a many-crate design until the boundaries are proven.

### Recommended Workspace Layout

```text
Cargo.toml
crates/
  svsim/         # public library crate
  svsim-render/  # optional PNG rendering helpers
  svsim-cli/     # thin CLI wrapper
```

Inside `svsim`, the implemented modules today are:

```text
svsim::compiler  # file/string compilation plus compile-only and JSON batch entry points
svsim::design    # compiled design handle plus hierarchy inspection
svsim::diag      # diagnostics and unsupported-feature errors
svsim::frontend  # sv-parser integration and lowering
svsim::hir       # owned executable subset
svsim::sim       # combinational + sequential engine
svsim::test      # JSON test execution and report types
svsim::validate  # internal semantic validation of lowered HIR
```

This keeps the public API simple for embedding while still separating concerns internally. A separate elaboration/value/compiled-IR split is still future work, not a current module boundary in the tree.

## Execution Model

### 1. Frontend

Parse SystemVerilog with `sv-parser`, then lower only the supported constructs into an owned HIR:

- modules
- ports and net/variable declarations
- packed ranges and unpacked memory arrays
- continuous assignments
- procedural blocks: `always_comb` and a first `always_ff @(posedge ...)` subset
- `if` / `else`
- `case` / `default`
- module instantiations with named port connections
- expressions: literals, identifiers, indexing, slicing, unary and binary ops, ternary

Current lowered subset in the tree today:

- continuous assignments
- `always_comb`
- `always_ff @(posedge <clock>)`
- fixed-size single-dimension unpacked memory declarations
- blocking and nonblocking procedural assignment
- `if` / `else`
- `case` / `default`
- memory element reads
- concatenation and replication expressions
- bit-select and part-select expressions
- unary bitwise-not
- binary `&`, `|`, `^`, `&&`, `||`, `==`, `!=`, `+`, `-`
- ternary expressions
- concatenated continuous/procedural assignment targets

Lowering should resolve syntax noise early so the simulator never touches parser-specific node shapes after compilation.

### 2. HIR

The HIR should describe executable intent, not source syntax.

Key HIR nodes:

- `Module`
- `Port`
- `NetDecl` / `VarDecl`
- `MemoryDecl`
- `Instance`
- `ContAssign`
- `ProcBlock`
- `Stmt`
- `Expr`
- `LValue`

Every node should carry source spans for diagnostics.

### 3. Elaboration

Elaboration should turn HIR modules into a compiled design:

- resolve module references from a design root
- assign stable instance paths
- compute widths and validate lvalues
- resolve named port bindings
- tag memory declarations as ROM or RAM where relevant
- build a per-instance signal table
- reject unsupported constructs before simulation starts

This is also where the current Python naming conventions should become explicit policies instead of hidden behavior:

- same-directory module resolution
- virtual-top-module directory resolution for `compile_str`

Make resolution and memory binding explicit through the library API rather than hardcoded globals.

Current status:

- the first slice of elaboration is now present as source-registration checks plus a compile-time validation pass over lowered HIR
- duplicate module definitions, duplicate declarations, duplicate instance names, identifier/memory resolution, lowered select bounds, constant out-of-range memory indices, unsupported `inout` / `ref` ports, input-port drive attempts, the most obvious invalid named-port bindings including missing child input connections, malformed legacy `rom_*` wrappers, missing legacy ROM backing files, and lowered width checks for declarations plus zero-width or overwide literals/concatenations/replications/concatenated lvalues are now rejected before simulation
- the first runtime width-normalization slices are now landed for self-determined ternary expressions plus explicit assignment and instance-port coercions inside the supported `1..=64` bit subset

### 4. Compiled IR

This section is future architecture, not the current runtime shape.

Do not interpret HIR directly on every cycle. Compile it into a simulation IR.

Recommended split:

- combinational expressions compiled into compact instruction trees or bytecode
- procedural blocks compiled into small statement programs
- per-module signal tables mapped to integer IDs
- precomputed dependency graphs for continuous assignments and `always_comb`

This is where most of the Rust-side performance win should come from.

### 5. Runtime

The runtime should support two execution modes:

- combinational evaluate-once
- cycle-stepped sequential simulation

Core semantics:

- continuous assign fixed-point convergence
- `always_comb` fixed-point convergence
- nonblocking assignment staging for `always_ff`
- blocking assignment immediate update within the current procedural context
- deterministic memory reads and single-element writes, with explicit programmatic preload/read hooks
- hierarchical instance stepping without recursive parser calls

For the current subset, cycle-stepped simulation is enough. Event-driven timing can stay out of scope for v1.
The current runtime interprets each `step()` call as one sampled active clock edge for supported `always_ff` blocks rather than as a free-running waveform simulation.

## Value Representation

The current designs are mostly 1 to 64 bits, with 32-bit support needed for the future RV32I milestone. Start with a width-aware bit-vector type optimized for that range.

Recommended value model:

- fast path: inline `u64`
- width mask stored with the value
- optional wide fallback later if widths above 64 become necessary

The current `64`-bit ceiling comes from this representation choice. Supporting wider designs requires widening `Value`, not merely increasing a validator constant.

Avoid binding the whole engine to `u128` or arbitrary precision on day one. The current corpus does not need it, and it will slow the hot path.

Memory model:

- ROM/RAM arrays stored as `Vec<Value>`
- explicit access mode metadata
- deterministic indexing and out-of-range diagnostics

## Embedding API

The library API should be the center of the design. Other languages can drive the CLI until a real FFI boundary is worth maintaining.

Sketch:

```rust
let design = svsim::Compiler::new()
    .add_search_path("parts/basic")
    .add_search_path("parts/overture")
    .compile_file("parts/basic/full_adder.sv")?;

let mut sim = design.instantiate_top()?;

let outputs = sim.eval_once(inputs)?;
let outputs = sim.step(inputs)?;
```

Useful embedded surfaces:

- compile from file
- compile from string with a virtual path that anchors relative dependency resolution
- configure module search paths
- preload or inspect ROM/RAM contents explicitly by instance path, including text-file initialization
- inspect ports and widths through the compiled HIR, and inspect instance hierarchy through `CompiledDesign::hierarchy()`
- evaluate combinational top modules
- step sequential top modules
- run JSON-backed regression suites through `CompiledDesign::run_json_file`
- batch-discover and run directory-backed JSON regressions through `Compiler::run_json_test_dir`
- reset simulator state by instantiating a fresh `SimulationSession`
- export traces for external visualization

The CLI should consume this API, not own separate logic.

## Image Strategy

Do not couple rendering to simulation. Keep it as an optional crate or feature, and do not make it part of the first parity milestone.

The simulator should emit structured results first:

- `TruthTable`
- `WaveTrace`
- `TestRunResult` (now partially in place as structured JSON test reports)

Those artifacts can be rendered later using:

- `image` for PNG output
- `imageproc` for lines, boxes, and simple geometry
- `ab_glyph` for text

This keeps the core simulator usable in headless or non-PNG embedding scenarios.

## Testing Strategy

Port the reference behavior before broadening scope.

### Golden Tests

Treat the current `parts/` tree as the compatibility suite:

- compile every `.sv` file
- run every matching `.json` test
- compare pass/fail counts against the reference project
- verify generated truth-table and waveform metadata before worrying about pixel-perfect images

### Unit Tests

Add focused tests for:

- width inference
- compile-time semantic validation failures
- bit slicing and concatenation
- replication
- ternary associativity
- blocking vs nonblocking assignment
- memory array reads and writes
- ROM auto-binding behavior
- hierarchical instance signal propagation

### Differential Tests

For the supported subset, run selected designs through both the Python reference and the Rust engine and compare outputs cycle-by-cycle.

## Performance Plan

The Rust version should be faster primarily because it compiles once and executes compact IR, not because it uses threads aggressively.

Priorities:

1. Parse once, elaborate once, simulate many times.
2. Replace string-keyed hot paths with stable integer signal IDs.
3. Precompute combinational evaluation order where possible.
4. Use fixed-width integer operations for the common case.
5. Parallelize batch regression with `rayon`, not single simulation steps.

Do not start with lock-heavy shared state or global caches. Embeddable code should prefer explicit ownership.

## Delivery Phases

### Phase 0: Workspace And Frontend Skeleton

- create Cargo workspace
- add `svsim`, `svsim-render`, `svsim-cli`
- integrate `sv-parser`
- compile files into HIR
- emit diagnostics for unsupported constructs

Exit criterion:
- all `parts/**/*.sv` parse successfully or fail with explicit unsupported-feature diagnostics

### Phase 1: Combinational Subset

- continuous assignments
- expressions, buses, slices, concatenation, replication, ternary
- named-port instantiation
- hierarchical combinational evaluation

Current progress:
- hierarchical combinational evaluation is implemented across continuous assignments, instances, `always_comb`, concatenation, replication, and concatenated lvalues
- `always_comb` convergence now uses each block's final assigned state, which fixes Overture-style blocks that seed an output before overriding it inside `case` or `if` logic
- child instance outputs are now memoized within each `settle_module` convergence pass when their inputs are unchanged, which removes the worst repeated subtree work from deep hierarchical combinational designs
- self-determined ternary expressions now preserve their fixed width at runtime via a shared lowered-expression width helper, which keeps concatenation and replication behavior aligned with validation
- logical shift operators `<<` / `>>` now lower, validate, constant-fold, and execute with the left operand's width, which unlocks a cleaner RV32I shift datapath in the demo corpus
- assignment and instance-port coercion now flow through an explicit shared runtime path, and child-instance cache keys now use the coerced child-visible input value rather than the raw parent expression bits
- grouped ANSI port declarations and initialized net declarations from the vector corpus are now lowered
- in-memory top-level compilation via `compile_str` is implemented, with dependency lookup anchored at the virtual path plus explicit search paths
- compilation now rejects several design-shape errors before simulation, including duplicate module definitions, undeclared identifiers, duplicate declarations, duplicate instance names, constant out-of-range memory indices, unsupported `inout` / `ref` ports, input-port drive attempts, unknown named ports, duplicate named-port bindings, missing child input bindings, non-lvalue output bindings, malformed legacy `rom_*` wrappers, missing legacy ROM backing files, and overwide declarations or concatenation-style value shapes that exceed the current 64-bit runtime limit
- callers can now inspect the compiled instance tree directly instead of reverse-engineering valid instance paths from raw HIR module summaries
- library-side JSON-backed combinational regression execution is implemented
- compile-only corpus coverage is now available for `*.sv` discovery even when runtime suites are not involved
- remaining work is continuing to grow compile-time validation into fuller elaboration and to replace iterative hot paths with more explicit evaluation order where it is justified

Exit criterion:
- parity for the combinational modules and tests in `parts/basic/` and `parts/testing/`

### Phase 2: Sequential Subset

- `always_ff`
- blocking and nonblocking scheduling
- multiple procedural blocks
- cycle stepping and reset

Current progress:
- `always_ff @(posedge <clock>)` lowering is implemented
- cycle-stepped state is preserved per instance across `step()` calls
- blocking and nonblocking assignment semantics work for the supported subset
- zero-initialized single-dimension memory reads, explicit memory preload/read APIs, text-file memory loading, and procedural single-element memory writes are implemented
- library-side JSON regression execution now covers sequential `test_cases`, including memory-backed suites
- remaining work is broader event controls and larger Overture sequential regressions

Exit criterion:
- parity for sequential register/counter tests and the math sequence stubs

### Phase 3: Memories And Overture

- JSON-driven memory binding through the library test runner
- Overture CPU regression suite
- batch regression entry points over the existing CLI regression mode

Current progress:
- library and CLI batch entry points now exist for both compile-only `*.sv` discovery and JSON regression discovery, runs stay sorted by source path, and JSON suites can explicitly reuse a shared source file through a `source` field
- legacy corpus compatibility for `rom_*` wrappers and `pgm_*` program harnesses now exists without reintroducing those naming conventions into the main library memory API, and malformed `rom_*` wrappers now fail at compile time instead of degrading into empty modules or late runtime errors
- measured Overture status now includes `41/41` clean compile-only source files and `43/43` passing JSON suites, including the two explicit-source `overture_cpu` program variants; the full multi-directory corpus snapshot is `126/126` compile-only in about `1.6s` and `128/128` JSON regressions in about `18.6s`
- remaining work is folding more in-range resolution and connection-shape checks into a single elaboration/validation layer now that the runtime coercion slices and constant memory-bounds checks are explicit, and tightening unsupported-construct diagnostics where wider coverage finds gaps

Exit criterion:
- parity for `parts/overture/` tests

### Phase 4: CLI And Structured Output

- truth table generation
- waveform capture
- machine-readable result output
- batch regression reporting polish beyond the current structured JSON summaries

Exit criterion:
- Rust CLI can replace the main workflows currently provided by `ref/pysvsim.py`

### Phase 5: Rendering And Embedding Polish

- stable public API review
- trace export format
- optional PNG rendering crate
- profiling and hotspot cleanup

Exit criterion:
- another app can compile and drive the simulator without shelling out to the CLI

## Deliberate Scope Cuts For V1

These should stay out unless the existing corpus forces them in:

- full preprocessor compatibility beyond what `sv-parser` already handles
- delays and event-driven timing simulation
- `always @(*)` generalization beyond the current subset
- parameterized modules and generates
- four-state logic (`X` / `Z`) semantics
- full synthesizable SystemVerilog coverage

## Recommended First Implementation Order

If starting immediately, the most pragmatic order is:

1. `sv-parser` integration plus owned HIR lowering
2. single-module combinational expression engine
3. hierarchical combinational elaboration
4. sequential scheduler
5. memories and Overture-specific binding rules
6. renderer and CLI

This gets to useful embeddable functionality quickly and avoids spending time on rendering before the simulator core is stable.

## Immediate Build Target

The first concrete implementation target should be:

1. compile the current `parts/` corpus with `sv-parser`
2. lower the supported subset into owned HIR
3. execute combinational modules and JSON tests
4. add sequential stepping
5. add explicit memory binding configuration
6. broaden Rust CLI regression coverage across Overture and add scalable batch execution

That library milestone, the compile-only and JSON corpus runners, the first compile-time semantic validation pass, the initial runtime width-normalization slices for self-determined ternary expressions plus assignment/instance-port coercion, and the current fixpoint engine are now in place. The next pragmatic target is to keep pulling in-range resolution checks into a single elaboration layer, then continue the runtime shift away from iterative hot paths toward a more explicit compiled evaluation model.

At that point the project has replaced the Python simulator for core use, even before image rendering exists.
