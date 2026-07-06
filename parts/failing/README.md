# Failing Corpus

This directory is the intentional negative corpus. It is gated by
`cargo test`: `corpus_failing_stays_red` in
`crates/svsim/tests/corpus_gate.rs` asserts that every suite here keeps
failing with its expected diagnostic. If you add, remove, or rename a suite —
or change a diagnostic message a suite depends on — update the tables in that
test in the same change.

- `missing_child_module.sv` + `missing_child_module.json`: compile/elaboration failure caused by an unresolved child module.
- `constant_one_mismatch.sv` + `constant_one_mismatch.json`: successful compile with an intentionally wrong JSON expectation.
- `syntax_error.sv` + `syntax_error.json`: parser failure from intentionally invalid SystemVerilog syntax.
- `malformed_json.sv` + `malformed_json.json`: successful compile with an intentionally broken JSON test file.
- `duplicate_instance_names.sv` + `duplicate_instance_names.json`: compile failure from repeated instance names in the same module.
- `constant_memory_index_oob.sv` + `constant_memory_index_oob.json`: compile failure from a constant memory access outside the declared array bounds.
- `param_override_frozen_range.sv` + `param_override_frozen_range.json`: elaboration failure from a non-default override of a parameter frozen into a port range at lowering time.
- `param_override_frozen_loop.sv` + `param_override_frozen_loop.json`: elaboration failure from a non-default override of a parameter frozen into an unrolled `for` loop bound.

To inspect the failure reports by hand:

```text
cargo run -q -p svsim-cli -- --compile-dir parts/failing
cargo run -q -p svsim-cli -- --json-test-dir parts/failing
./test-fails.sh
```

Keep this directory out of the all-green expectations: every other `parts/`
directory except `parts/roms` (data assets) is the green corpus, gated by the
same `corpus_gate.rs`.
