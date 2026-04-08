# Source Map

This wiki is synthesized from repository files. The files listed here are the raw source layer that wiki pages should link back to when making concrete claims.

| Category | Primary sources | Why they matter |
| --- | --- | --- |
| Repo contract | [../../AGENTS.md](../../AGENTS.md), [../../Cargo.toml](../../Cargo.toml) | Defines the intended project structure, green corpus, and workspace shape. |
| Architecture strategy | [../../docs/rust-port-plan.md](../../docs/rust-port-plan.md) | Best single narrative description of the rewrite goals, current subset, and intended architecture. |
| Recent project snapshot | [../../docs/progress/progress-report-2026-03-20.md](../../docs/progress/progress-report-2026-03-20.md) | Captures the most recent checked-in progress report before this wiki bootstrap. |
| Porting lessons | [../../docs/sap1-port-compromises.md](../../docs/sap1-port-compromises.md) | Documents where imported hardware still needs simulator-facing compromises. |
| Core library surface | [../../crates/svsim/src/lib.rs](../../crates/svsim/src/lib.rs), [../../crates/svsim/src/compiler.rs](../../crates/svsim/src/compiler.rs), [../../crates/svsim/src/design.rs](../../crates/svsim/src/design.rs), [../../crates/svsim/src/sim.rs](../../crates/svsim/src/sim.rs), [../../crates/svsim/src/hir.rs](../../crates/svsim/src/hir.rs), [../../crates/svsim/src/test.rs](../../crates/svsim/src/test.rs), [../../crates/svsim/src/diag.rs](../../crates/svsim/src/diag.rs) | The real product boundary lives here. |
| CLI surface | [../../crates/svsim-cli/src/main.rs](../../crates/svsim-cli/src/main.rs) | Shows how the library is intended to be wrapped and exposed. |
| Render status | [../../crates/svsim-render/src/lib.rs](../../crates/svsim-render/src/lib.rs) | Confirms that rendering is still explicitly deferred. |
| Corpus docs | [../../parts/rv32i/README.md](../../parts/rv32i/README.md), [../../parts/picorv32/README.md](../../parts/picorv32/README.md), [../../parts/sap1/README.md](../../parts/sap1/README.md), [../../parts/failing/README.md](../../parts/failing/README.md), [../../parts/roms/roms.md](../../parts/roms/roms.md) | Explain the intent of the main demo, real-world, and negative corpus directories. |
| Self-documenting sources | [../../parts/simple8/simple8.sv](../../parts/simple8/simple8.sv), [../../parts/simple8/simple8_programmable.sv](../../parts/simple8/simple8_programmable.sv) | These files carry their own design-level documentation and fill the lack of a dedicated README. |

## Notes

- Prefer fresh command output over historical docs when recording current status.
- Historical progress reports are still useful for the "why" behind a change even when their exact counts drift.
