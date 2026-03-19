# PicoRV32 Sample Programs

This directory now contains a small executable PicoRV32 corpus in addition to the upstream `picorv32.v` source and the original smoke harness.

## Included Harness

- `picorv32_program_harness.sv`: wraps `picorv32` with:
  - a 64-word ROM at address range `0x0000_0000` to `0x0000_00ff`
  - a 16-word RAM at address range `0x0000_0100` to `0x0000_013f`
  - byte-lane RAM writes using `mem_wstrb`
  - visible outputs for `trap`, the native memory bus, the first four RAM words, and last-store metadata

## Sample Programs

- `demo_add_chain.json`: straight-line `ADDI` chain that accumulates `1..10` and stores `55`
- `demo_add_chain_long.json`: longer `ADDI` chain that accumulates `1..20` and stores `210`
- `demo_shift_pack.json`: `SLLI` + `ADDI` chain that packs `0x01020304`
- `picorv32_smoke.json`: original minimal store smoke test

Each JSON suite preloads a different ROM text file and then clocks the harness until the post-store `trap` state is visible.

The current executable subset is narrower than compile coverage: straight-line PicoRV32 programs that end in a single final store are stable here today, while multi-access data programs and taken-branch loops are not yet part of the checked-in green corpus.

## Run

```bash
cargo run -p svsim-cli -- --json-test-dir parts/picorv32
```
