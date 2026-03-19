@echo off
echo Building...
cargo build -q -p svsim-cli --release
echo Testing...
.\target\release\svsim.exe --json-test-dir parts/basic > docs/tests/report-parts-basic.json
.\target\release\svsim.exe --json-test-dir parts/overture > docs/tests/report-parts-overture.json
.\target\release\svsim.exe --json-test-dir parts/picorv32 > docs/tests/report-parts-picorv32.json
.\target\release\svsim.exe --json-test-dir parts/rv32i > docs/tests/report-parts-rv32i.json
.\target\release\svsim.exe --json-test-dir parts/testing > docs/tests/report-parts-testing.json
