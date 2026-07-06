# SAP-2 Port

The `sap2` corpus is the first checked-in proof that `svsim` can support a more natural shared-bus machine structure without reopening public/top-level `inout`.

## What The Port Proves

- Internal whole-net `inout` leaf ports can alias parent nets and participate in resolved shared-bus behavior.
- Four-state outputs can expose floating `z` and contention-driven `x` through harness-visible outputs when the design needs them.
- The existing SAP-family program corpus can be reused while moving the internal machine organization closer to a traditional shared bus.

## What Is Still Constrained

*(Updated 2026-07-06: two earlier constraints have since lifted — public/top-level
`inout` is now supported and regression-tested (`sap2_inout_top`), and `sap2` is
part of the gated green corpus enforced by
[`corpus_gate.rs`](../../crates/svsim/tests/corpus_gate.rs).)*

- Internal `inout` is currently limited to zero-delay, whole-net bindings rather than arbitrary lvalue targets.

## Checked-In Coverage

- [`../../parts/sap2/sap2.sv`](../../parts/sap2/sap2.sv): runnable machine that preserves the existing harness-visible top contract.
- Generated `sap2_*.json` suites and `*_ram.txt` program images: reused SAP-family workloads that check visible machine behavior.
- [`../../parts/sap2/sap2_bus_semantics.sv`](../../parts/sap2/sap2_bus_semantics.sv) and [`../../parts/sap2/sap2_bus_semantics.json`](../../parts/sap2/sap2_bus_semantics.json): focused floating/contention smoke coverage.
- [`../../parts/sap2/gen_svsim.py`](../../parts/sap2/gen_svsim.py): refreshes the local SAP-2 corpus from checked-in SAP-1 assets.

## Relationship To SAP-1

- [sap1.md](./sap1.md) captures the older compromise-heavy path: explicit muxed bus, harness-visible control ports, and simulator-friendly rewrites.
- `sap2` keeps the same harness-visible contract, but it moves leaf bus participants back onto a shared `inout` fabric that drives `z` when inactive.
- Together, the two ports now show both the original import friction and the first meaningful reduction in that friction.

## Sources

- [../../plan.md](../../plans/completed/plan-sap2-inout.md)
- [../../parts/sap2/README.md](../../parts/sap2/README.md)
- [../../parts/sap2/sap2.sv](../../parts/sap2/sap2.sv)
- [../../parts/sap2/sap2_bus_semantics.sv](../../parts/sap2/sap2_bus_semantics.sv)
- [../../parts/sap2/gen_svsim.py](../../parts/sap2/gen_svsim.py)
