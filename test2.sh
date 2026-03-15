#!/usr/bin/env bash
echo "Building..."
cargo build -q -p svsim-cli --release
echo "Testing..."

# ./target/release/svsim --json-test parts/basic/full_adder.json parts/basic/full_adder.sv

# ./target/release/svsim --json-test-dir parts/basic > report-parts-basic.json 
./target/release/svsim --json-test-dir parts/overture > report-parts-overture.json 

echo "Done"