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
  - a 4-word RAM window at address range `0x0000_0100` to `0x0000_010f`
  - byte-lane RAM writes using `mem_wstrb`
  - one-cycle delayed visible store commits, so trap-on-store cases can suppress RAM mutation in the harness
  - visible outputs for `trap`, the native memory bus, the first four RAM words, and last-store metadata

## Sample Programs

- `demo_add_chain.json`: straight-line `ADDI` chain that accumulates `1..10` and stores `55`
- `demo_add_chain_long.json`: longer `ADDI` chain that accumulates `1..20` and stores `210`
- `demo_branch_taken.json`: taken `BEQ` skips an untaken `ADDI`, then stores `42` before trapping
- `demo_compare_branch.json`: `SLT`, `SLTU`, `BLT`, and `BLTU` prove signed and unsigned ordering diverge as expected before three visible stores
- `demo_instr_misaligned_jalr.json`: `jalr` targets address `0x0000_0006`, proving PicoRV32 enters its instruction-misalignment trap path before any visible store commits
- `demo_jump_link.json`: `jal` and masked `jalr` both write their link registers, skip untaken work, and still store `42`
- `demo_load_roundtrip.json`: `SW` followed by `LW` feeds a derived second store, proving load-backed dataflow through the RAM window
- `demo_misaligned_load.json`: misaligned `LW` traps before any visible store can commit
- `demo_misaligned_store.json`: misaligned `SW` traps without mutating the visible RAM window
- `demo_subword_mem.json`: `LB` / `LBU` plus `LH` / `LHU` sign behavior and `SB` / `SH` lane writes through the PicoRV32 RAM window
- `demo_shift_pack.json`: `SLLI` + `ADDI` chain that packs `0x01020304`
- `demo_two_store.json`: back-to-back visible stores that write `1` then `2` before trapping
- `picorv32_smoke.json`: original minimal store smoke test
- `demo_branch_taken.txt`: ROM backing file for the checked-in taken-branch regression
- `demo_jump_link.txt`: ROM backing file for the checked-in jump/link regression
- `demo_two_store.txt`: ROM backing file for the checked-in two-store regression

Each JSON suite preloads a different ROM text file and then clocks the harness until the post-store `trap` state is visible.

Sequential JSON traces can now also name internal hierarchical signals such as `uut.cpu_state` or `uut.reg_pc`, which makes it practical to debug PicoRV32 control-path issues without adding temporary ports to the harness.

The executable subset is intentionally narrower than compile coverage. The checked-in green PicoRV32 runtime corpus is currently:

- the original smoke harness
- straight-line sample programs that end in a single final store
- a taken conditional-branch sample that skips untaken work and lands on the correct masked target
- a compare-heavy sample that proves `slt` / `sltu` and `blt` / `bltu` disagree in the expected signed-vs-unsigned way before trapping
- a jump/link sample that proves both `jal` and masked `jalr` targets plus link-register writeback
- an instruction-misaligned `jalr` sample that enters PicoRV32 trap state before any visible store commits
- a load-backed sample that stores `17`, reloads it with `lw`, derives `42`, and stores the result into the next RAM word
- explicit misaligned `lw` and `sw` samples that trap before any visible RAM mutation reaches the harness
- a subword-memory sample that mutates RAM with `sb` / `sh`, proves `lb` sign extension, proves `lbu` zero extension, and stores the signed-vs-unsigned halfword delta `0x00010000`
- a two-store continuation sample that writes consecutive RAM words before trapping

The checked-in runtime surface now covers post-store continuation, taken conditional branches, signed-vs-unsigned compare control flow, jump/link control flow, one instruction-path misalignment trap (`jalr`), a load-backed datapath case, subword memory execution, and explicit misaligned load/store traps through `demo_two_store.json`, `demo_branch_taken.json`, `demo_compare_branch.json`, `demo_jump_link.json`, `demo_instr_misaligned_jalr.json`, `demo_load_roundtrip.json`, `demo_subword_mem.json`, `demo_misaligned_load.json`, and `demo_misaligned_store.json`. The next bounded PicoRV32 runtime target is the remaining branch-side instruction-path trap case: a taken misaligned conditional branch target.

## Run

```bash
cargo run -p svsim-cli -- --json-test-dir parts/picorv32
```
