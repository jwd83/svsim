Sketch SAP-3 corpus landed as Slice 4 of [`plan-next.md`](../../plan-next.md).

[`sap3.sv`](./sap3.sv) keeps the shared-bus architecture from
[`parts/sap2`](../sap2/) and the harness contract from
[`parts/sap1`](../sap1/), but widens the simulated CPU:

- Adds three logical ALU ops (`AND`, `OR`, `XOR`) at opcodes `0xB`, `0xC`,
  and `0xD`.
- Replaces the dedicated `out_r` register with a memory-mapped output port
  inside the `memory` module. The address register widens to 5 bits and the
  top bit (address `0x10`) selects the output port; `OUT` now executes as
  "select output port, then write bus".
- Widens the microcode word from 16 bits to 20 bits to carry four new
  control signals (`alu_op_and`, `alu_op_or`, `alu_op_xor`,
  `en_select_output_port`).
- Reuses the `register_tile`, `register_pc_tile`, `register_instr_tile`,
  and `bus_driver` modules verbatim from `sap2.sv`.

What is checked in here:

- [`sap3.sv`](./sap3.sv): the widened CPU.
- [`assemble.py`](./assemble.py): SAP-3 assembler (adds `and`, `or`, `xor`
  mnemonics).
- [`make_micro_instr.py`](./make_micro_instr.py): 20-bit microcode
  definition, parallel to [`parts/sap2/make_micro_instr.py`](../sap2/make_micro_instr.py).
- [`gen_svsim.py`](./gen_svsim.py): single-file generator that emits
  `sap3_microcode.txt`, per-program `sap3_*_ram.txt`, and the
  cycle-indexed `sap3_*.json` suites.
- [`examples/`](./examples/): `.s` programs covering the new opcodes
  (`logic_mask.s`, `parity.s`) alongside ported variants of the SAP-1
  classics (`add3to42.s`, `fib.s`) that now write through the memory-mapped
  output port.

## Running

Regenerate the corpus from source:

```text
python3 parts/sap3/gen_svsim.py
```

Run the checked-in suite through the CLI:

```text
cargo run -q -p svsim-cli -- --json-test-dir parts/sap3
```

No simulator changes were required for this slice — the same elaboration,
four-state net resolver, and top-level `inout` path landed for Slices 1-3
handles the widened microcode and new peripheral shape.
