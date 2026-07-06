# Runtime And State

`SimulationSession` is the main runtime handle. It owns an elaborated structural runtime rooted at the compiled top module and exposes the operations used both by embedding callers and by the JSON harness.

## State Model

- Runtime state is hierarchical: each module instance gets its own `ModuleState`, but parent and child ports can alias the same underlying runtime object when elaboration says they should.
- A module state tracks parameter values, signal bindings, memories, previous clock values, legacy ROM info when applicable, and child instances.
- Runtime objects are typed by storage kind, so nets and variables can behave differently even when they share the same bit width.
- HDL-facing values use `LogicValue` / `LogicBits`, which preserve `0`, `1`, `x`, and `z`. `BitValue` is still available as the narrower 2-state convenience layer.

## Two Execution Modes

- `eval_once(inputs)`: settle a combinational design to a fixed point and return top-level outputs.
- `step(inputs)`: execute one sampled sequential step, then run combinational settle on the resulting state before returning outputs.
- `*_2state` wrappers preserve the older "fail if any `x` / `z` escapes" behavior for callers that still want a strictly 2-state boundary.

## Current Semantics

- Continuous assignments and `always_comb` blocks settle to a fixed point across hierarchy.
- `always_ff` supports `posedge` clocks and optional `posedge` async reset.
- Blocking assignment updates immediately within the active procedural context.
- Nonblocking assignment in clocked blocks is staged.
- Nets resolve per bit, with storage-kind-aware rules and drive strengths handled by the resolver.
- Expression and control evaluation preserve four-state behavior instead of silently collapsing `x` / `z`.
- Memories can be preloaded programmatically or from text files and then inspected again through runtime APIs.
- Top-level outputs, JSON reports, and CLI output can round-trip literal four-state values.
- `read_signal` can inspect internal hierarchical signals without temporary debug ports, which is especially useful for CPU harnesses.

## Useful Runtime Hooks

- `load_memory_words`
- `load_memory_words_2state`
- `load_memory_file`
- `read_memory_word`
- `read_signal`

These hooks are why the project can keep the library API explicit while still driving fairly rich JSON regression suites.

## Current Limits

- The runtime is cycle-stepped, not a full event-driven waveform simulator.
- Only the supported `always_comb` and `always_ff @(posedge ...)` style procedural surface is executed.
- Public/top-level `inout` is supported as of 2026-04-16. A JSON test step may drive an `inout` input as either a two-state number (a contender, e.g. `"bus": 165`) or as `"8'bz"` / `"zzzzzzzz"` to release the bus; omitting the input entirely is equivalent to releasing it. Observed `inout` ports are reported back with their resolved four-state value after settle.
- Internal `inout` currently only works for whole-net parent bindings; it is not a general arbitrary-lvalue feature.
- Gate/switch primitives and pull devices are still deferred even though zero-delay net resolution now exists.
- Timing controls, richer testbench semantics, and more general event controls are still outside the supported subset.
- Imported designs may still need harness-friendly ports or explicit memory preload paths; [../ports/sap1.md](../ports/sap1.md) shows the older compromise-heavy path, while [../ports/sap2.md](../ports/sap2.md) shows the first shared-bus recovery built on the new runtime.

## Sources

- [../../crates/svsim/src/elaborate.rs](../../crates/svsim/src/elaborate.rs)
- [../../crates/svsim/src/logic_value.rs](../../crates/svsim/src/logic_value.rs)
- [../../crates/svsim/src/net_resolve.rs](../../crates/svsim/src/net_resolve.rs)
- [../../crates/svsim/src/sim/](../../crates/svsim/src/sim/)
- [../../crates/svsim/src/design.rs](../../crates/svsim/src/design.rs)
- [../../crates/svsim/src/test.rs](../../crates/svsim/src/test.rs)
- [../../crates/svsim/src/width.rs](../../crates/svsim/src/width.rs)
- [../../crates/svsim/src/bit_value.rs](../../crates/svsim/src/bit_value.rs)
