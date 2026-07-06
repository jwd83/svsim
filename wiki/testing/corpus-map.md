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

- Since 2026-07-06 every directory above except `parts/failing` and
  `parts/roms` is part of the gated green surface:
  [`crates/svsim/tests/corpus_gate.rs`](../../crates/svsim/tests/corpus_gate.rs)
  runs each one under `cargo test` and fails on any red suite (or on a
  missing/empty directory). The older four-directory "official" set
  (`basic`, `testing`, `overture`, `rv32i`) is a historical distinction.
- `parts/picorv32` remains the most advanced stress target: the upstream core
  compiles, and its harness exercises a curated executable slice.
- `parts/failing` is the negative corpus and must stay out of the green gate.

## Practical Commands

```text
cargo test                       # green-corpus gate + unit + CLI tests
./test.sh                        # regenerate docs/tests/report-parts-*.json
cargo run -q -p svsim-cli -- --json-test-dir parts/sap3   # one dir ad hoc
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
