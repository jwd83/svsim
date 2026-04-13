# Index

## Start Here

- [overview.md](./overview.md): what `svsim` is, what it is trying to become, and how to approach the repo.
- [status/current-state.md](./status/current-state.md): the latest verified repository snapshot captured by this wiki.
- [roadmap/inout-and-sap2-milestone.md](./roadmap/inout-and-sap2-milestone.md): the current four-state / internal-`inout` / `sap2` milestone and what is still intentionally locked.
- [sources/source-map.md](./sources/source-map.md): the raw-source inventory this wiki is built from.
- [log.md](./log.md): append-only maintenance history for the wiki itself.

## Architecture

- [architecture/workspace-map.md](./architecture/workspace-map.md): crates, top-level directories, and the main Rust modules, including elaboration and four-state runtime support.
- [architecture/compiler-pipeline.md](./architecture/compiler-pipeline.md): how source files move through parsing, lowering, validation, elaboration, and execution.
- [architecture/runtime-and-state.md](./architecture/runtime-and-state.md): how `SimulationSession` uses elaborated runtime objects, four-state values, and net resolution.

## Testing And Corpus

- [testing/corpus-map.md](./testing/corpus-map.md): a directory-by-directory map of the `parts/` corpus, including the new auxiliary `sap2` slice.

## Ports And Case Studies

- [ports/cpu-corpora.md](./ports/cpu-corpora.md): how Overture, RV32I, PicoRV32, SAP-1, SAP-2, and Simple8 fit together as learning and stress-test targets.
- [ports/sap1.md](./ports/sap1.md): the clearest write-up of where imported Verilog still needs simulator concessions.
- [ports/sap2.md](./ports/sap2.md): the first runnable shared-bus case study built on internal whole-net `inout` support.

## Status And Roadmap

- [roadmap/inout-and-sap2-milestone.md](./roadmap/inout-and-sap2-milestone.md): the active milestone summary derived from `plan.md`, recent code, and verified command output.
- [roadmap/open-edges.md](./roadmap/open-edges.md): the main unsupported constructs, architecture gaps, and next bounded follow-ups.
