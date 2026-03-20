# PicoRV32 Sample Programs

This directory now contains a small executable PicoRV32 corpus in addition to the upstream `picorv32.v` source and the original smoke harness.

## Compile Coverage

- `cargo run -p svsim-cli -- --compile-dir parts/picorv32` now measures all three checked-in source files here:
  - `picorv32.v`
  - `picorv32_program_harness.sv`
  - `picorv32_smoke.sv`
- The compile-only status is currently `3/3` clean files.

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
- `demo_two_store.txt`: manual two-store ROM probe that reproduces the current post-store continuation gap without being part of the green JSON corpus yet

Each JSON suite preloads a different ROM text file and then clocks the harness until the post-store `trap` state is visible.

Sequential JSON traces can now also name internal hierarchical signals such as `uut.cpu_state` or `uut.reg_pc`, which makes it practical to debug PicoRV32 control-path issues without adding temporary ports to the harness.

The executable subset is intentionally narrower than compile coverage. The checked-in green PicoRV32 runtime corpus is currently:

- the original smoke harness
- straight-line sample programs that end in a single final store

Current scratch traces narrow the next runtime gap further: the simulator can reach the first visible store in small multi-store probes, but PicoRV32 post-store continuation and taken-branch loops are still not part of the checked-in green corpus. `demo_two_store.txt` is the checked-in manual repro for that first continuation bug.

## Run

```bash
cargo run -p svsim-cli -- --json-test-dir parts/picorv32
```
