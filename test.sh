#!/usr/bin/env bash
# Regenerates the committed corpus reports under docs/tests/.
# The gating check is `cargo test` (see crates/svsim/tests/corpus_gate.rs);
# this script only refreshes the published report artifacts.
set -euo pipefail
echo "Building..."
cargo build -q -p svsim-cli --release
echo "Testing..."
./target/release/svsim --json-test-dir parts/basic > docs/tests/report-parts-basic.json
./target/release/svsim --json-test-dir parts/overture > docs/tests/report-parts-overture.json
./target/release/svsim --json-test-dir parts/picorv32 > docs/tests/report-parts-picorv32.json
./target/release/svsim --json-test-dir parts/rv32i > docs/tests/report-parts-rv32i.json
./target/release/svsim --json-test-dir parts/sap1 > docs/tests/report-parts-sap1.json
./target/release/svsim --json-test-dir parts/sap2 > docs/tests/report-parts-sap2.json
./target/release/svsim --json-test-dir parts/sap3 > docs/tests/report-parts-sap3.json
./target/release/svsim --json-test-dir parts/simple8 > docs/tests/report-parts-simple8.json
./target/release/svsim --json-test-dir parts/testing > docs/tests/report-parts-testing.json
echo "Done"
