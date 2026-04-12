This directory now holds the first runnable `sap2` corpus slice for Phase 7.
It keeps the same harness-visible top contract as [`parts/sap1`](../sap1/),
but the internal machine in [`sap2.sv`](./sap2.sv) moves back to a shared bus
with leaf `inout` participants that drive `z` when inactive.

What is checked in here:

- the copied assembler and example programs from [`parts/sap1`](../sap1/)
- a local microcode image at [`sap2_microcode.txt`](./sap2_microcode.txt)
- generated `sap2_*.json` program suites that mirror the current `sap1` corpus
- generated `sap2_*_ram.txt` program images
- a focused floating/contention smoke suite:
  [`sap2_bus_semantics.sv`](./sap2_bus_semantics.sv) and
  [`sap2_bus_semantics.json`](./sap2_bus_semantics.json)
- a generator script, [`gen_svsim.py`](./gen_svsim.py), that refreshes the
  local SAP-2 corpus from the checked-in SAP-1 assets

## Running

Run the full `sap2` directory through the Rust CLI:

```text
cargo run -q -p svsim-cli -- --json-test-dir parts/sap2
```

That exercises both the harness-compatible `machine` top in
[`sap2.sv`](./sap2.sv) and the focused bus semantics smoke suite.
