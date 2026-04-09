This directory is the phase-2 skeleton for the future `sap2` corpus. It is
intentionally non-gating today: the checked-in source and generator are
scaffolding for the later `inout` / resolved-bus work, not a finished machine.

What is already here:

- the shared assembler and example programs copied from [`parts/sap1`](../sap1/)
- a copied microcode image placeholder at [`sap2_microcode.txt`](./sap2_microcode.txt)
- an adapted generator scaffold at [`gen_svsim.py`](./gen_svsim.py)
- a placeholder harness-facing top module at [`sap2.sv`](./sap2.sv)

What is deliberately missing for now:

- checked-in `sap2_*.json` regression suites
- a structurally shared internal bus
- real `inout` participants and contention/floating behavior

When later phases land, this directory will keep the same harness-visible top
contract as `sap1` while moving the internal machine structure closer to the
original shared-bus design.
