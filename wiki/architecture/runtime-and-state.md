# Runtime And State

`SimulationSession` is the main runtime handle. It owns a tree of per-module state rooted at the compiled top module and exposes the operations used both by embedding callers and by the JSON harness.

## State Model

- Runtime state is hierarchical: each module instance gets its own `ModuleState`.
- A module state tracks persisted signals, memories, previous clock values, legacy ROM info when applicable, and child instances.
- Bit values use the limb-backed `BitValue` type, so the runtime can work with widths that go beyond a single `u64`.

## Two Execution Modes

- `eval_once(inputs)`: settle a combinational design to a fixed point and return top-level outputs.
- `step(inputs)`: execute one sampled sequential step, then run combinational settle on the resulting state before returning outputs.

## Current Semantics

- Continuous assignments and `always_comb` blocks settle to a fixed point across hierarchy.
- `always_ff` supports `posedge` clocks and optional `posedge` async reset.
- Blocking assignment updates immediately within the active procedural context.
- Nonblocking assignment in clocked blocks is staged.
- Memories can be preloaded programmatically or from text files and then inspected again through runtime APIs.
- `read_signal` can inspect internal hierarchical signals without temporary debug ports, which is especially useful for CPU harnesses.

## Useful Runtime Hooks

- `load_memory_words`
- `load_memory_file`
- `read_memory_word`
- `read_signal`

These hooks are why the project can keep the library API explicit while still driving fairly rich JSON regression suites.

## Current Limits

- The runtime is cycle-stepped, not a full event-driven waveform simulator.
- Only the supported `always_comb` and `always_ff @(posedge ...)` style procedural surface is executed.
- Timing controls, richer testbench semantics, and more general event controls are still outside the supported subset.
- Imported designs may still need harness-friendly ports or explicit memory preload paths; [../ports/sap1.md](../ports/sap1.md) is the clearest example.

## Sources

- [../../crates/svsim/src/sim.rs](../../crates/svsim/src/sim.rs)
- [../../crates/svsim/src/design.rs](../../crates/svsim/src/design.rs)
- [../../crates/svsim/src/test.rs](../../crates/svsim/src/test.rs)
- [../../crates/svsim/src/width.rs](../../crates/svsim/src/width.rs)
- [../../crates/svsim/src/bit_value.rs](../../crates/svsim/src/bit_value.rs)
