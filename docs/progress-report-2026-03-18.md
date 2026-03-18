# Rewrite Progress Report

Date: March 18, 2026

## Executive Summary

**picorv32 compiles.** The open-source RISC-V softcore `picorv32.v` — a 3,000-line production Verilog design — now parses, lowers, validates, and produces a full HIR output through svsim. This is the first external real-world CPU design to compile through the Rust simulator, and it required closing four distinct language-feature gaps in a single session:

1. Verilog-2001 `always @*` / `always @(posedge clk)` sensitivity lists
2. Reduction operators (`|`, `&`, `^` as unary)
3. Parameter-expression range bounds (`[regindex_bits-1:0]`)
4. Multiply operator (`*`)

All existing tests remain green. The measured repository state now sits at `98/98` Rust tests, `136/136` compile-only green source files, and `153/153` passing JSON regression suites across `parts/basic`, `parts/testing`, `parts/overture`, and `parts/rv32i`.

## What Changed Today

### always @* and always @(posedge clk) mapping
- Added `lower_always_generic()` in `sv_parser.rs` that inspects the `EventControl` sensitivity list inside a bare `always` block
- `@*` and `@(*)` map to `ProcBlockKind::AlwaysComb`
- `@(posedge signal)` maps to `ProcBlockKind::AlwaysFf { clock }`
- No HIR, sim, or validator changes required — the existing infrastructure handles both kinds
- Added `parts/testing/always_star_comb.sv/.json` and `parts/testing/always_posedge_ff.sv/.json`

### Reduction operators
- Added `ReductionAnd`, `ReductionOr`, `ReductionXor` to `UnaryOp` in HIR
- Mapped unary `|`, `&`, `^` in the parser
- All three produce 1-bit results: OR = `!is_zero()`, AND = `bits == mask(width)`, XOR = popcount parity
- Evaluation added in `sim.rs`, `validate.rs` (const-eval), and `width.rs`
- Added `parts/testing/reduction_ops.sv/.json` (8 test vectors)

### Parameter-expression range bounds
- Threaded `&[ParameterDecl]` through the entire range-lowering chain: `lower_constant_range` → `lower_packed_dimensions` → `lower_data_type_range` → `lower_data_type_or_implicit_range` → `lower_ansi_port_declaration` → `lower_data_declaration` → `lower_net_declaration`
- Added `lower_usize_constant_expression_with_params` handling `Binary`, `Ternary`, `PsParameter`, and `ConstantFunctionCall` (sv-parser parses bare identifiers in constant expressions as zero-arg function calls)
- Added `const_eval_param_value` for recursive HIR Expr evaluation against known parameters, supporting all binary/unary/ternary operators
- Added `parts/testing/param_expr_range.sv/.json`

### Multiply operator
- Added `BinaryOp::Mul` to HIR
- Added `wrapping_mul` to `BitValue` with schoolbook multi-limb multiplication
- Mapped `*` in `lower_binary_operator`
- Width follows add/sub convention: `left_width.max(right_width)`
- Added `parts/testing/multiply.sv/.json` (7 test vectors including overflow)

## Verified Current State

- `cargo test`: pass (98 tests: 90 unit + 8 integration)
- `cargo run -p svsim-cli -- parts/picorv32/picorv32.v`: **compiles successfully**, emits about 370KB of HIR JSON, and lowers 8 modules with `picorv32_wb` as the discovered top module
- Compile-only multi-directory corpus: `136/136` (`parts/basic` `44/44`, `parts/testing` `50/50`, `parts/overture` `41/41`, `parts/rv32i` `1/1`)
- JSON regression multi-directory corpus: `153/153` (`parts/basic` `44/44`, `parts/testing` `50/50`, `parts/overture` `43/43`, `parts/rv32i` `16/16`)
- picorv32 HIR still carries `28` `unsupported` entries across 6 of the 8 lowered modules, so compile-clean does not yet mean full simulation coverage

## picorv32 Remaining Work

The design compiles to HIR but cannot simulate end-to-end yet. The current blockers come from each module's `unsupported` list in the emitted HIR:

| Gap | Current HIR message | Seen in |
|-----|---------------------|---------|
| Parameterized child instantiation | `parameterized module instantiations are not supported yet` | `picorv32_axi`, `picorv32_wb` |
| Non-constant selects | `only constant bit and part select indices are supported` | `picorv32_pcpi_fast_mul` |
| Procedural local declarations | `procedural blocks with local declarations are not supported yet` | `picorv32` |
| Task declarations | `task declarations are not supported yet` | `picorv32` |
| 4-state literal syntax | `x/z numeric literal digits are not supported yet` | `picorv32`, `picorv32_pcpi_div` |
| Remaining expression / statement / module-item forms | `primary expression is not supported yet`, `statement is outside the current executable subset`, `statement attributes are not supported yet`, `module item is outside the current executable subset`, `unary operator is not supported yet`, `literal is outside the current executable subset` | `picorv32`, `picorv32_pcpi_mul`, `picorv32_pcpi_fast_mul` |

## Commands Run

```text
cargo test
cargo build
cargo run -p svsim-cli -- parts/picorv32/picorv32.v
cargo run -p svsim-cli -- parts/testing/always_star_comb.sv --json-test parts/testing/always_star_comb.json
cargo run -p svsim-cli -- parts/testing/always_posedge_ff.sv --json-test parts/testing/always_posedge_ff.json
cargo run -p svsim-cli -- parts/testing/reduction_ops.sv --json-test parts/testing/reduction_ops.json
cargo run -p svsim-cli -- parts/testing/param_expr_range.sv --json-test parts/testing/param_expr_range.json
cargo run -p svsim-cli -- parts/testing/multiply.sv --json-test parts/testing/multiply.json
cargo run -p svsim-cli -- --compile-dir parts/basic --compile-dir parts/testing --compile-dir parts/overture --compile-dir parts/rv32i
cargo run -p svsim-cli -- --json-test-dir parts/basic --json-test-dir parts/testing --json-test-dir parts/overture --json-test-dir parts/rv32i
```
