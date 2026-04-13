# CPU Corpora

The repository has several CPU-shaped designs, and they serve different purposes. Treating them as interchangeable misses a lot of the structure of the project.

## Overture

- Lives in [`../../parts/overture`](../../parts/overture).
- Best educational hierarchical CPU stack in the green corpus.
- Good target when changing core sequential semantics, hierarchy handling, or the legacy `pgm_*` convenience path.

## RV32I Demo Core

- Lives in [`../../parts/rv32i`](../../parts/rv32i).
- Compact 32-bit core designed to fit the current executable subset cleanly.
- Best target when extending control flow, trap semantics, memory behavior, and 32-bit datapath support without importing the full complexity of a production core.

## PicoRV32

- Lives in [`../../parts/picorv32`](../../parts/picorv32).
- Acts as the main real-world compile target and a curated runtime stress target.
- Useful when you want to know whether a change helps with actual open-source SystemVerilog instead of only repo-native teaching designs.

## SAP-2

- Lives in [`../../parts/sap2`](../../parts/sap2).
- New runnable auxiliary corpus that keeps the harness-visible `sap1` contract while moving the internal machine back toward a shared-bus structure.
- Best target when touching internal `inout`, net resolution, floating/contending buses, or the question "does this richer runtime still preserve the visible educational CPU behavior?"

## Simple8

- Lives in [`../../parts/simple8`](../../parts/simple8).
- Tiny single-cycle teaching CPU with self-documenting source comments.
- Good for lightweight instruction-set and memory-flow experiments.

## SAP-1

- Lives in [`../../parts/sap1`](../../parts/sap1).
- Less about broad compatibility gating and more about documenting import friction and harness tradeoffs.
- See [sap1.md](./sap1.md) for the key lessons and [sap2.md](./sap2.md) for the first partial reversal of those compromises.

## Sources

- [../../parts/overture](../../parts/overture)
- [../../parts/rv32i/README.md](../../parts/rv32i/README.md)
- [../../parts/picorv32/README.md](../../parts/picorv32/README.md)
- [../../parts/sap2/README.md](../../parts/sap2/README.md)
- [../../parts/simple8/simple8.sv](../../parts/simple8/simple8.sv)
- [../../parts/sap1/README.md](../../parts/sap1/README.md)
