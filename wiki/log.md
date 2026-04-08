# Log

## [2026-04-08] bootstrap | Initial wiki scaffold

- Created the initial wiki structure under `wiki/` with a schema, index, log, overview, architecture pages, testing notes, port case studies, a roadmap page, and a raw-source map.
- Built the first pass primarily from `AGENTS.md`, `docs/rust-port-plan.md`, `docs/sap1-port-compromises.md`, `docs/progress/progress-report-2026-03-20.md`, `parts/*/README.md`, and the `crates/svsim` Rust modules.
- Verified `cargo test`: pass (`131` `svsim` tests and `8` CLI tests; no failures).
- Verified `cargo run -q -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i`: pass (`136/136`).
- Verified `cargo run -q -p svsim-cli -- --compile-dir parts/picorv32`: pass (`3/3`).
- Verified `cargo run -q -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i --json-test-dir parts/picorv32`: pass (`166/166`).
