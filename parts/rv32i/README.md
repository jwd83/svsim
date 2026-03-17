# RV32I Demo Corpus

This directory adds a small RV32I-flavored corpus that fits the current executable subset of `svsim`.

## Included CPU

- `rv32i_cpu.sv`: a compact 32-bit demo core with:
  - 32-bit byte-addressed `pc`
  - 32 × 32-bit register file
  - 64-word instruction memory (`imem`)
  - 64-word data memory (`dmem`)
  - real RV32I encodings for `ADDI`, `ADD`, `SUB`, `AND`, `OR`, `XOR`, `LUI`, `LW`, `SW`, `BEQ`, `BNE`, and `JAL`

## Demo Convention

- Each JSON suite uses `"source": "rv32i_cpu.sv"` and binds a different `.txt` image into `imem`.
- `jal x0, 0` is treated as a demo halt instruction so tests can assert a stable stopped state.
- `LW`/`SW` are word-aligned and index `dmem` with `addr[7:2]`.

## Demo Programs

- `demo_add_store.json`: arithmetic, store, load, halt
- `demo_sum_loop.json`: counted loop using `bne`
- `demo_branch_jal.json`: taken `beq` plus `jal` link-register behavior

## Run

```bash
cargo run -p svsim-cli -- --json-test-dir parts/rv32i
```
