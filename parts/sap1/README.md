This directory contains the maintained `svsim` port of Ben Eater's SAP-1
computer. The legacy standalone Verilog/testbench flow has been removed; the
supported entry point here is [`sap1.sv`](./sap1.sv) plus the JSON harness
inputs in this directory.

The port keeps the same overall machine shape, with a few intentional
differences to fit the current simulator:

- no `inout` bus ports
- no standalone testbench modules
- memory and microcode are loaded through the `svsim` harness data files
- a few instruction/microcode details differ from the original videos

## Running

Run the SAP-1 suites through the Rust CLI:

```text
cargo run -q -p svsim-cli -- --json-test-dir parts/sap1
```

That executes the checked-in programs against the `machine` top module in
[`sap1.sv`](./sap1.sv).
