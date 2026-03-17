# RV32I Demo Corpus

This directory adds a small RV32I-flavored corpus that fits the current executable subset of `svsim`.

## Included CPU

- `rv32i_cpu.sv`: a compact 32-bit demo core with:
  - 32-bit byte-addressed `pc`
  - 32 × 32-bit register file
  - 64-word instruction memory (`imem`)
  - 64-word data memory (`dmem`)
  - trap outputs for `ecall`, `ebreak`, and illegal instructions
  - real RV32I encodings for `LUI`, `AUIPC`, `JAL`, `JALR`
  - arithmetic and logical ops: `ADDI`, `SLTI`, `SLTIU`, `XORI`, `ORI`, `ANDI`, `SLLI`, `SRLI`, `SRAI`, `ADD`, `SUB`, `SLL`, `SLT`, `SLTU`, `XOR`, `SRL`, `SRA`, `OR`, `AND`
  - control-flow ops: `BEQ`, `BNE`, `BLT`, `BGE`, `BLTU`, `BGEU`
  - memory ops: `LB`, `LBU`, `LH`, `LHU`, `LW`, `SB`, `SH`, `SW`
  - misc/system ops: `FENCE`, `FENCE.I`, `ECALL`, `EBREAK`

## Demo Convention

- Each JSON suite uses `"source": "rv32i_cpu.sv"` and binds a different `.txt` image into `imem`.
- `jal x0, 0` is treated as a demo halt instruction so tests can assert a stable stopped state.
- `trap=1` with `trap_cause=11`, `3`, or `2` models `ecall`, `ebreak`, or an illegal instruction respectively.
- `dmem` is still word-backed; byte and halfword accesses select lanes within `dmem[addr[7:2]]`.
- The demo suites keep `LH`/`LHU`/`SH` within one 32-bit word and do not model misaligned traps.

## Demo Programs

- `demo_add_store.json`: arithmetic, store, load, halt
- `demo_sum_loop.json`: counted loop using `bne`
- `demo_branch_jal.json`: taken `beq` plus `jal` link-register behavior
- `demo_shift_ops.json`: logical and arithmetic shifts, including register-form `shamt[4:0]` masking
- `demo_compare_control.json`: `AUIPC`, `JALR`, signed/unsigned compare ops, and the rest of the simple branch family
- `demo_subword_mem.json`: `LB`/`LBU`/`LH`/`LHU` plus `SB`/`SH` lane updates
- `demo_system_misc.json`: `FENCE`, `FENCE.I`, and `ECALL`
- `demo_breakpoint.json`: `EBREAK`
- `demo_illegal_instr.json`: illegal-instruction trapping


## Run

```bash
cargo run -p svsim-cli -- --json-test-dir parts/rv32i
```
