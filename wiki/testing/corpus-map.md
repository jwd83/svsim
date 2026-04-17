# Corpus Map

The `parts/` tree is the compatibility surface for the rewrite. Different directories play different roles; not every checked-in design is meant to be part of the same green gate.

| Directory | Role | Notes |
| --- | --- | --- |
| [`../../parts/basic`](../../parts/basic) | Foundational green corpus | Gates, muxes, adders, registers, ROM wrappers, and small sequential blocks. |
| [`../../parts/testing`](../../parts/testing) | Feature-pinning green corpus | Focused cases for parsing, widths, params, memories, shifts, and harness behavior. |
| [`../../parts/overture`](../../parts/overture) | Hierarchical green corpus | Educational 8-bit CPU stack and `pgm_*` programs. |
| [`../../parts/rv32i`](../../parts/rv32i) | 32-bit demo CPU green corpus | Compact RV32I-style core that exercises more realistic control, memory, and trap behavior. |
| [`../../parts/picorv32`](../../parts/picorv32) | Real-world compile target plus curated runtime corpus | Upstream `picorv32.v` compiles; harnessed sample programs exercise a bounded executable slice. |
| [`../../parts/sap1`](../../parts/sap1) | Imported design case study | Maintained SAP-1 port used to document current simulator friction. |
| [`../../parts/sap2`](../../parts/sap2) | Auxiliary shared-bus runtime corpus | Runnable SAP-family follow-on that restores internal shared-bus structure with focused `inout` / floating / contention coverage. `sap2_register_tile.{sv,json}` isolates the register-tile partitioning and `sap2_inout_top.{sv,json}` exposes the bus as a top-level `inout` to prove the public-`inout` boundary. |
| [`../../parts/sap3`](../../parts/sap3) | Richer SAP sketch | Extends the SAP-2 shared-bus CPU with AND/OR/XOR ops, a memory-mapped output port at address `0x10`, and a 20-bit microcode word. Exists to prove the simulator side of the SAP port story is complete — no simulator changes were required to run it. |
| [`../../parts/simple8`](../../parts/simple8) | Teaching/demo CPU corpus | Tiny self-documenting CPU with a programmable harness. |
| [`../../parts/failing`](../../parts/failing) | Negative corpus | Intentionally failing parser, compile, and JSON cases. |
| [`../../parts/roms`](../../parts/roms) | ROM text-file conventions | Documents the plain-text ROM format used by harnessed designs. |

## Green Surface vs Auxiliary Surface

- The official all-green compatibility surface is `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i`.
- `parts/picorv32` is compile-green and also has a curated executable harness surface, but it is still best thought of as an advanced demo and stress target rather than the base compatibility contract.
- `parts/sap2` is a runnable auxiliary corpus that specifically targets the new four-state/internal-`inout` work; it is useful, but it is not yet part of the official green contract.
- `parts/sap1` and `parts/simple8` remain valuable design examples without being the canonical gate for every change.

## Practical Commands

```text
cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i
cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i
cargo run -q -p svsim-cli -- --json-test-dir parts/picorv32
cargo run -q -p svsim-cli -- --json-test-dir parts/sap2
cargo run -q -p svsim-cli -- --json-test-dir parts/sap3
cargo run -q -p svsim-cli -- --compile-dir parts/failing
cargo run -q -p svsim-cli -- --json-test-dir parts/failing
```

## Sources

- [../../AGENTS.md](../../AGENTS.md)
- [../../parts/rv32i/README.md](../../parts/rv32i/README.md)
- [../../parts/picorv32/README.md](../../parts/picorv32/README.md)
- [../../parts/sap1/README.md](../../parts/sap1/README.md)
- [../../parts/sap2/README.md](../../parts/sap2/README.md)
- [../../parts/sap3/README.md](../../parts/sap3/README.md)
- [../../parts/failing/README.md](../../parts/failing/README.md)
- [../../parts/roms/roms.md](../../parts/roms/roms.md)
