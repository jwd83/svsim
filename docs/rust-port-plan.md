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
- replace implicit `rom_*` and `pgm_*` naming conventions with explicit configuration

## Current Status

Current implementation status as of March 14, 2026:

- Cargo workspace created with `svsim`, `svsim-cli`, and `svsim-render`
- `sv-parser` integrated for file-based parsing
- owned HIR covers source files, module summaries, continuous assignments, instantiations, `always_comb`, and a first `always_ff @(posedge ...)` subset
- library API started with `Compiler`, `CompiledDesign`, and `SimulationSession`
- CLI can parse a SystemVerilog file and emit JSON describing discovered modules
- `SimulationSession::eval_once` can execute hierarchical combinational designs with fixed-point convergence across continuous assignments, module instances, and a basic `always_comb` subset
- `SimulationSession::step` now maintains per-instance state and can advance hierarchical designs using `always_ff @(posedge <clock>)` blocks with nonblocking assignment staging
- current procedural subset: blocking assignments in combinational blocks, nonblocking assignments in `always_ff`, `if` / `else`, `case` / `default`, and `begin` / `end` statement blocks without local declarations
- current expression subset adds logical `&&` / `||`, equality `==` / `!=`, and arithmetic `+` / `-` on top of literals, identifiers, slices, ternary expressions, and bitwise operators
- current sequential limits: only `posedge` event controls are lowered, `always_ff` clock expressions must be local identifiers, and blocking assignments inside `always_ff` are still rejected
- memories, explicit elaboration, general event controls, and test-runner/render integration are still pending

## Compatibility Target

The first meaningful milestone is feature parity with the subset exercised by the current repository:

- `125` SystemVerilog files under `parts/`
- hierarchical modules built from gates up through the Overture CPU
- combinational logic with buses, slices, concatenation, replication, arithmetic, comparisons, and ternary expressions
- `always_comb`
- `always_ff @(posedge clk)` with blocking and nonblocking assignment
- ROM and RAM style memory arrays
- JSON-backed combinational and sequential tests
- structured truth-table and waveform results

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
- Image output: `image`
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

Inside `svsim`, keep these modules:

```text
svsim::frontend  # sv-parser integration and lowering
svsim::hir       # owned executable subset
svsim::elab      # hierarchy resolution and instance expansion
svsim::value     # bit-vector values and memory storage
svsim::sim       # combinational + sequential engine
svsim::test      # JSON test execution
svsim::diag      # diagnostics and unsupported-feature errors
```

This keeps the public API simple for embedding while still separating concerns internally.

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
- blocking and nonblocking procedural assignment
- `if` / `else`
- `case` / `default`
- bit-select and part-select expressions
- unary bitwise-not
- binary `&`, `|`, `^`, `&&`, `||`, `==`, `!=`, `+`, `-`
- ternary expressions

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

Make resolution and memory binding explicit through the library API rather than hardcoded globals.

### 4. Compiled IR

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
- deterministic memory reads and writes
- hierarchical instance stepping without recursive parser calls

For the current subset, cycle-stepped simulation is enough. Event-driven timing can stay out of scope for v1.
The current runtime interprets each `step()` call as one sampled active clock edge for supported `always_ff` blocks rather than as a free-running waveform simulation.

## Value Representation

The current designs are mostly 1 to 64 bits, with 32-bit support needed for the future RV32I milestone. Start with a width-aware bit-vector type optimized for that range.

Recommended value model:

- fast path: inline `u64`
- width mask stored with the value
- optional wide fallback later if widths above 64 become necessary

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
- compile from string
- configure module search paths
- configure ROM/RAM bindings
- inspect ports, widths, and instance hierarchy
- evaluate combinational top modules
- step sequential top modules
- reset simulator state
- export traces for external visualization

The CLI should consume this API, not own separate logic.

## Image Strategy

Do not couple rendering to simulation. Keep it as an optional crate or feature, and do not make it part of the first parity milestone.

The simulator should emit structured results first:

- `TruthTable`
- `WaveTrace`
- `TestRunResult`

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
- nonblocking assignment staging works for the supported subset
- remaining work is broader event controls, memories, and larger Overture sequential regressions

Exit criterion:
- parity for sequential register/counter tests and the math sequence stubs

### Phase 3: Memories And Overture

- packed memory arrays
- explicit ROM/RAM bindings
- Overture CPU regression suite

Exit criterion:
- parity for `parts/overture/` tests

### Phase 4: CLI And Structured Output

- truth table generation
- waveform capture
- machine-readable result output
- batch test runner with parallel execution

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
6. run Overture regressions through the Rust CLI

At that point the project has replaced the Python simulator for core use, even before image rendering exists.
