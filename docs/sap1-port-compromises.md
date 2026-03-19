# SAP-1 Port Compromises

This note records the changes we made to get the Ben Eater SAP-1 design
running under the current `svsim` Rust harness, and what we would want long
term so ports like this can stay closer to their original Verilog.

Current maintained entrypoint:

- [`parts/sap1/sap1.sv`](/Users/jared/projects/svsim/parts/sap1/sap1.sv)

Current harness assets:

- [`parts/sap1/sap1_fib.json`](/Users/jared/projects/svsim/parts/sap1/sap1_fib.json)
- [`parts/sap1/sap1_add3to42.json`](/Users/jared/projects/svsim/parts/sap1/sap1_add3to42.json)
- [`parts/sap1/sap1_multiply.json`](/Users/jared/projects/svsim/parts/sap1/sap1_multiply.json)
- [`parts/sap1/gen_svsim.py`](/Users/jared/projects/svsim/parts/sap1/gen_svsim.py)

## Summary

The SAP-1 port is runnable today, but it is not a "drop the original Verilog
in and go" success story. We had to reshape the design around current `svsim`
constraints:

- remove the original standalone testbench flow
- replace the shared `inout` bus with an explicit muxed bus
- load RAM and microcode through harness memory injection instead of in-source
  initialization
- expose harness-friendly top-level I/O instead of relying on simulator tasks
  and side effects
- rewrite some source forms into the narrower frontend subset

None of those are fatal for a port, but they are exactly the kinds of changes
we should need less of over time.

## Compromises We Made

### 1. We removed the original standalone testbench

The original flow used a dedicated testbench and simulator-driven execution.
That does not fit the current `svsim` support level well, so the maintained
version is just the synthesizable machine plus JSON harness suites.

What changed:

- deleted the old `tb_cpu.v` testbench
- deleted the old `Makefile`/Icarus flow
- replaced self-running simulation with `--json-test-dir parts/sap1`

Why:

- current frontend support is intentionally narrower than full Verilog
- the old testbench relied on simulator-oriented constructs that are not part
  of the core execution path we support today

Long term target:

- compile and run simple Verilog/SystemVerilog testbenches directly, so we do
  not need a custom harness for every imported design

### 2. We replaced the shared `inout` bus with an explicit mux

Ben Eater-style designs naturally lean on a shared bus. The old Verilog used an
`inout` bus model. The maintained port in
[`sap1.sv`](/Users/jared/projects/svsim/parts/sap1/sap1.sv) uses a plain wire
plus an explicit priority mux:

```verilog
assign bus = en_read_external ? external_value
           : en_read_alu      ? alu
           : en_read_instr    ? { 4'b0, out_reg_instr[3:0] }
           : en_read_mem      ? out_mem
           : en_read_a        ? out_reg_a
           : en_read_pc       ? out_reg_pc
           : 0;
```

Why:

- the old compile path failed on unsupported `inout` ports
- explicit muxing is much easier for the current simulator than modeling
  bidirectional bus drivers and net resolution

Cost:

- the source is less faithful to the original bus style
- bus contention behavior is no longer represented structurally
- we lose a natural path toward `z`/resolved-net semantics

Long term target:

- support `inout` ports and multi-driver net resolution well enough that small
  bus-oriented machines can keep their original structure

### 3. We moved RAM and ROM initialization into the harness

The maintained port leaves the memory arrays declared in Verilog, but their
contents are injected from JSON:

- module `memory`, memory `data`
- module `rom`, memory `data`

See [`parts/sap1/sap1_fib.json`](/Users/jared/projects/svsim/parts/sap1/sap1_fib.json)
for the current shape:

```json
"memory_init": [
  { "module": "memory", "memory": "data", "file": "sap1_fib_ram.txt" },
  { "module": "rom", "memory": "data", "file": "sap1_microcode.txt" }
]
```

Why:

- we did not keep the original simulator-side init path
- the removed legacy flow depended on old-school simulator usage rather than
  the Rust harness model
- harness memory injection gives us deterministic setup without extra source
  constructs

Cost:

- the design is not self-contained
- program loading and microcode loading live outside the HDL
- running the design requires the `svsim` JSON layer, not just the Verilog file

Long term target:

- support enough initialization semantics that simple designs can use source
  level memory init or standard simulator idioms without being rewritten around
  external harness preload files

### 4. We exposed harness-specific top-level controls and observations

The maintained top module is a harness-friendly API:

- `input wire en_read_external`
- `input wire [7:0] external_value`
- `output wire [7:0] out_reg_out`
- `output reg halted`

Why:

- the JSON runner needs explicit input/output surfaces
- we cannot rely on testbench-only tasks or simulator exit behavior to drive
  tests

Cost:

- the top-level shape is more "test harness contract" than "natural hardware
  interface"
- `halted` is an exposed latch for test observation rather than just simulation
  termination
- `en_read_external` and `external_value` are there primarily for harnessing,
  not because the machine itself naturally wants that public interface

Long term target:

- better support for richer testbench stimulus and termination so imported
  designs need fewer artificial ports

### 5. We rewrote some source forms into the narrower accepted subset

Even where the hardware behavior stayed the same, we still had to bias toward a
clean subset of Verilog/SystemVerilog. The legacy source/testbench combination
used forms that are not yet fully supported in the normal `svsim` path.

Examples from the old flow and the current compile boundary:

- ordered port connections
- variable declarations with initializers
- `initial` blocks
- delay-based `always` constructs

The maintained SAP-1 file was kept inside the subset that currently compiles
and simulates reliably.

Why:

- current frontend/simulator coverage is deliberately focused on the constructs
  needed for the green corpus

Cost:

- ports from outside the project still need cleanup before they fit naturally
- "works in Icarus" still does not imply "works unchanged in `svsim`"

Long term target:

- expand frontend and simulation coverage until small existing Verilog designs
  stop needing mechanical normalization before they can run here

### 6. We rely on generated expectations instead of original self-checking behavior

The SAP-1 suites are generated by
[`gen_svsim.py`](/Users/jared/projects/svsim/parts/sap1/gen_svsim.py), which
simulates the machine in Python and emits JSON sequences with expected
`out_reg_out` and `halted` observations.

Why:

- the current harness wants explicit per-step expectations
- this gives stable regression tests for long-running programs like Fibonacci

Cost:

- there is now an external oracle in the loop
- the tests are less like "run the original design in a generic simulator" and
  more like "run our port against generated expectations"

Long term target:

- keep JSON regressions where useful, but reduce the amount of design-specific
  scaffolding needed to make imported machines executable

## What We Want Less Of

The general direction is not "eliminate the harness." The direction is:

- imported bus-oriented designs should need fewer structural rewrites
- standard simulator-oriented memory initialization should need fewer custom
  preload files
- simple testbenches should compile with less manual conversion
- design ports should not need extra harness-only plumbing unless we choose it
  for convenience

If we make progress in those areas, a future SAP-1 style import should look
much more like:

1. bring in the original design
2. add a small compatibility wrapper at most
3. run it under `svsim`

instead of:

1. rewrite bus structure
2. rewrite initialization strategy
3. replace the execution model with a custom harness contract

## Concrete Long-Term Capability Targets

The most useful simulator/frontend improvements for reducing port friction are:

- `inout` port support and net resolution
- better support for `initial` constructs
- support for standard memory initialization idioms such as `$readmemh`
- support for ordered port connections
- support for declaration initializers where they are semantically simple
- testbench execution features that cover basic clocks, stop conditions, and
  non-synthesizable harness glue

Those features are not all equally important, but together they are the
difference between "ported design" and "design we had to partially rewrite."
