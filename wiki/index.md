# Index

## Start Here

- [overview.md](./overview.md): what `svsim` is, what it is trying to become, and how to approach the repo.
- [status/current-state.md](./status/current-state.md): the latest verified repository snapshot captured by this wiki.
- [sources/source-map.md](./sources/source-map.md): the raw-source inventory this wiki is built from.
- [log.md](./log.md): append-only maintenance history for the wiki itself.

## Architecture

- [architecture/workspace-map.md](./architecture/workspace-map.md): crates, top-level directories, and the main Rust module responsibilities.
- [architecture/compiler-pipeline.md](./architecture/compiler-pipeline.md): how source files move through parsing, lowering, validation, design construction, and execution.
- [architecture/runtime-and-state.md](./architecture/runtime-and-state.md): how `SimulationSession` works, what `eval_once` and `step` mean, and where current limits live.

## Testing And Corpus

- [testing/corpus-map.md](./testing/corpus-map.md): a directory-by-directory map of the `parts/` corpus and what each family is for.

## Ports And Case Studies

- [ports/cpu-corpora.md](./ports/cpu-corpora.md): how Overture, RV32I, PicoRV32, SAP-1, and Simple8 fit together as learning and stress-test targets.
- [ports/sap1.md](./ports/sap1.md): the clearest write-up of where imported Verilog still needs simulator concessions.

## Status And Roadmap

- [roadmap/open-edges.md](./roadmap/open-edges.md): the main unsupported constructs, architecture gaps, and next bounded follow-ups.
