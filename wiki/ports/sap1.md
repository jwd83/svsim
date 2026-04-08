# SAP-1 Port

The SAP-1 port is the clearest written example of where `svsim` still asks imported designs to meet the simulator halfway.

## What The Port Proves

- The current simulator can run a nontrivial educational machine end to end through JSON harnesses.
- The current simulator still does not let you drop in arbitrary legacy Verilog and expect it to run unchanged.

## Main Compromises

- The original standalone testbench flow was removed in favor of JSON-driven harness execution.
- The shared `inout` bus was replaced with an explicit muxed bus.
- RAM and microcode initialization moved out of source-level behavior and into harness memory injection.
- The maintained top module exposes harness-friendly control and observation ports.
- Some source forms were rewritten into the currently supported subset.

## Why This Page Matters

SAP-1 is less important as a target in itself than as a catalog of current simulator friction:

- bus-oriented designs want `inout` and resolved-net behavior
- imported designs want more natural memory initialization
- testbench-driven designs want richer simulator-side harness semantics

If a future feature reduces the number of SAP-1-specific compromises, it is probably improving the product in a broadly useful way.

## Sources

- [../../docs/sap1-port-compromises.md](../../docs/sap1-port-compromises.md)
- [../../parts/sap1/README.md](../../parts/sap1/README.md)
- [../../parts/sap1/sap1.sv](../../parts/sap1/sap1.sv)
