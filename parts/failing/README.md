# Failing Corpus

This directory is a negative corpus for manual regression checks.

- `missing_child_module.sv` + `missing_child_module.json`: compile/elaboration failure caused by an unresolved child module.
- `constant_one_mismatch.sv` + `constant_one_mismatch.json`: successful compile with an intentionally wrong JSON expectation.
- `syntax_error.sv` + `syntax_error.json`: parser failure from intentionally invalid SystemVerilog syntax.
- `malformed_json.sv` + `malformed_json.json`: successful compile with an intentionally broken JSON test file.
- `duplicate_instance_names.sv` + `duplicate_instance_names.json`: compile failure from repeated instance names in the same module.

Use this directory when you want failure reports on purpose:

```text
cargo run -q -p svsim-cli -- --compile-dir parts/failing
cargo run -q -p svsim-cli -- --json-test-dir parts/failing
uv run ref/pysvsim.py parts/failing/
```

For the all-green compatibility corpus, keep using `parts/basic`, `parts/testing`, and `parts/overture`.
