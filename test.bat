@echo off
echo Building...
cargo build -q -p svsim-cli --release
echo Testing...
.\target\release\svsim.exe --json-test-dir parts/basic > report-parts-basic.json
.\target\release\svsim.exe --json-test-dir parts/overture > report-parts-overture.json
.\target\release\svsim.exe --json-test-dir parts/rv32i > report-parts-rv32i.json
.\target\release\svsim.exe --json-test-dir parts/testing > report-parts-testing.json
