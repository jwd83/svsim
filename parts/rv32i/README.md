# RV32I Demo Corpus

This directory adds a small RV32I-flavored corpus that fits the current executable subset of `svsim`.

## Included CPU

- `rv32i_cpu.sv`: a compact 32-bit demo core with:
  - 32-bit byte-addressed `pc`
  - 32 × 32-bit register file
  - 64-word instruction memory (`imem`)
  - 64-word data memory (`dmem`)
  - trap outputs for instruction-address misalignment, `ecall`, `ebreak`, illegal instructions, and misaligned halfword or word data accesses
  - real RV32I encodings for `LUI`, `AUIPC`, `JAL`, `JALR`
  - arithmetic and logical ops: `ADDI`, `SLTI`, `SLTIU`, `XORI`, `ORI`, `ANDI`, `SLLI`, `SRLI`, `SRAI`, `ADD`, `SUB`, `SLL`, `SLT`, `SLTU`, `XOR`, `SRL`, `SRA`, `OR`, `AND`
  - control-flow ops: `BEQ`, `BNE`, `BLT`, `BGE`, `BLTU`, `BGEU`
  - memory ops: `LB`, `LBU`, `LH`, `LHU`, `LW`, `SB`, `SH`, `SW`
  - misc/system ops: `FENCE`, `FENCE.I`, `ECALL`, `EBREAK`

## Demo Convention

- Each JSON suite uses `"source": "rv32i_cpu.sv"` and binds a different `.txt` image into `imem`.
- `jal x0, 0` is treated as a demo halt instruction so tests can assert a stable stopped state.
- `trap=1` with `trap_cause=0`, `11`, `3`, `2`, `4`, or `6` models an instruction-address misalignment trap, `ecall`, `ebreak`, an illegal instruction, a misaligned load, or a misaligned store respectively.
- `dmem` is still word-backed; byte and halfword accesses select lanes within `dmem[addr[7:2]]`.
- `LH`/`LHU` and `SH` still operate only within a single 32-bit word, but misaligned halfword and word data accesses now trap instead of silently aliasing lanes.
- `imem` is still word-backed, so taken `BEQ`/`BNE`/`BLT`/`BGE`/`BLTU`/`BGEU`, `JAL`, and `JALR` targets with `pc[1:0] != 0` now trap instead of silently aliasing `imem[pc[7:2]]`.

## Demo Programs

- `demo_add_store.json`: arithmetic, store, load, halt
- `demo_sum_loop.json`: counted loop using `bne`
- `demo_branch_jal.json`: taken `beq` plus `jal` link-register behavior
- `demo_shift_ops.json`: logical and arithmetic shifts, including register-form `shamt[4:0]` masking
- `demo_compare_control.json`: `AUIPC`, `JALR`, signed/unsigned compare ops, and the rest of the simple branch family
- `demo_instr_misaligned_branch.json`: taken branch instruction-address misalignment trapping with cause `0`
- `demo_instr_misaligned_jalr.json`: `JALR` instruction-address misalignment trapping with cause `0`
- `demo_subword_mem.json`: `LB`/`LBU`/`LH`/`LHU` plus `SB`/`SH` lane updates
- `demo_system_misc.json`: `FENCE`, `FENCE.I`, and `ECALL`
- `demo_breakpoint.json`: `EBREAK`
- `demo_illegal_instr.json`: illegal-instruction trapping
- `demo_misaligned_load.json`: misaligned `LW` trapping with cause `4`
- `demo_misaligned_store.json`: misaligned `SW` trapping with cause `6`

## Run

```bash
cargo run -p svsim-cli -- --json-test-dir parts/rv32i
```
