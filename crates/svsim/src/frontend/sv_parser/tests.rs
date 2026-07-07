use std::path::PathBuf;

use super::SvParserFrontend;
use crate::hir::{
    AssignmentKind, BinaryOp, Expr, LValue, NetKind, NumericLiteral, ProcBlockKind, Stmt,
    StorageKind,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

#[test]
fn parse_file_collects_module_name() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/basic/full_adder.sv"))
        .expect("parse full_adder");

    assert_eq!(source.modules.len(), 1);
    assert_eq!(source.modules[0].name, "full_adder");
    assert_eq!(source.modules[0].instantiations.len(), 3);
    assert_eq!(
        source.modules[0].instantiations[0].module_name,
        "half_adder"
    );
    assert_eq!(source.modules[0].instantiations[0].instance_name, "u_half1");
    assert!(
        source.modules[0].instantiations[0]
            .parameter_overrides
            .is_empty()
    );
}

#[test]
fn parse_file_lowers_named_parameter_overrides() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/picorv32/picorv32.v"))
        .expect("parse picorv32");

    let module = source
        .modules
        .iter()
        .find(|module| module.name == "picorv32_wb")
        .expect("picorv32_wb module");
    let instance = module
        .instantiations
        .iter()
        .find(|instance| instance.instance_name == "picorv32_core")
        .expect("picorv32_core instance");

    assert!(
        module.unsupported.is_empty(),
        "unexpected unsupported entries: {:?}",
        module.unsupported
    );
    assert!(
        instance
            .parameter_overrides
            .iter()
            .any(|param| param.parameter_name == "ENABLE_COUNTERS")
    );
    assert!(
        instance
            .parameter_overrides
            .iter()
            .any(|param| param.parameter_name == "PROGADDR_RESET")
    );
}

#[test]
fn parse_file_lowers_assignments_and_ports() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/basic/ternary_mux.sv"))
        .expect("parse ternary mux");

    let module = &source.modules[0];
    assert_eq!(module.ports.len(), 4);
    assert_eq!(module.continuous_assignments.len(), 1);
    assert!(module.unsupported.is_empty());
}

#[test]
fn parse_str_lowers_modules_from_virtual_path() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/top.sv"),
            "module top(input logic a, output logic y); assign y = ~a; endmodule\n",
        )
        .expect("parse virtual source");

    assert_eq!(source.path, PathBuf::from("/virtual/design/top.sv"));
    assert_eq!(source.modules.len(), 1);
    let module = &source.modules[0];
    assert_eq!(module.name, "top");
    assert!(module.unsupported.is_empty());
    assert_eq!(module.ports.len(), 2);
    assert_eq!(module.continuous_assignments.len(), 1);
}

#[test]
fn parse_str_attaches_spans_to_unsupported_diagnostics() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/span_probe.sv"),
            concat!(
                "module span_probe(input a, output logic y);\n",
                "    always_comb begin\n",
                "        repeat (3) begin\n",
                "            y = a;\n",
                "        end\n",
                "    end\n",
                "endmodule\n"
            ),
        )
        .expect("parse span probe");

    let module = &source.modules[0];
    assert_eq!(module.unsupported.len(), 1);
    let diagnostic = &module.unsupported[0];
    assert!(
        diagnostic
            .message
            .contains("loop statement is outside the current executable subset"),
        "unexpected message: {}",
        diagnostic.message
    );
    let span = diagnostic
        .span
        .as_ref()
        .expect("unsupported diagnostic must carry a span");
    assert_eq!(span.line, 3, "span should point at the repeat loop");
}

#[test]
fn parse_str_records_frozen_parameters_per_construct() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/frozen_probe.sv"),
            r#"
module frozen_probe #(
parameter WIDTH = 4,
parameter DEPTH = 8,
parameter N = 2,
parameter SEL = 1,
parameter GEN = 0,
parameter IDX = 1,
parameter REP = 2,
parameter OFFSET = 3
) (
input  [WIDTH-1:0] a,
input  [3:0] data,
output logic [3:0] s,
output logic sel_out,
output gen_out,
output [1:0] r,
output [7:0] q
);
logic [3:0] mem [0:DEPTH-1];
integer i;

assign r = {REP{1'b1}};
assign q = {4'b0000, data} + OFFSET;
wire picked = data[IDX];

generate
    if (GEN) begin : g
        assign gen_out = picked;
    end else begin : h
        assign gen_out = ~picked;
    end
endgenerate

always_comb begin
    s = 4'b0000;
    for (i = 0; i < N; i = i + 1) begin
        s = data;
    end
    if (SEL == 1) sel_out = 1'b1;
    else sel_out = 1'b0;
end
endmodule
"#,
        )
        .expect("parse frozen probe");

    assert_eq!(source.modules.len(), 1);
    let module = &source.modules[0];
    assert!(
        module.unsupported.is_empty(),
        "unexpected unsupported diagnostics: {:?}",
        module.unsupported
    );

    let frozen = &module.frozen_parameters;
    let get = |name: &str| frozen.get(name).map(String::as_str);
    assert_eq!(get("WIDTH"), Some("a packed declaration range"));
    assert_eq!(get("DEPTH"), Some("an unpacked dimension"));
    assert_eq!(get("N"), Some("a procedural `for` loop bound"));
    assert_eq!(get("SEL"), Some("a constant-folded `if` condition"));
    assert_eq!(get("GEN"), Some("a generate `if` condition"));
    assert_eq!(get("IDX"), Some("a constant bit select"));
    assert_eq!(get("REP"), Some("a replication count"));
    assert!(
        !frozen.contains_key("OFFSET"),
        "expression-only parameter must not be frozen: {frozen:?}"
    );
}

#[test]
fn parse_file_lowers_always_comb_blocks() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/basic/mux_4to1_comb.sv"))
        .expect("parse mux_4to1_comb");

    let module = &source.modules[0];
    assert_eq!(module.proc_blocks.len(), 1);
    assert!(module.unsupported.is_empty());
}

#[test]
fn parse_file_lowers_always_ff_blocks() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/basic/register_8bit.sv"))
        .expect("parse register_8bit");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.proc_blocks.len(), 1);
    assert_eq!(
        module.proc_blocks[0].kind,
        ProcBlockKind::AlwaysFf {
            clock: "clk".into(),
            async_reset: None,
        }
    );
    match &module.proc_blocks[0].body {
        Stmt::Block(statements) => match &statements[0] {
            Stmt::If { then_branch, .. } => match then_branch.as_ref() {
                Stmt::Block(statements) => match &statements[0] {
                    Stmt::Assign { kind, .. } => {
                        assert_eq!(*kind, AssignmentKind::Nonblocking);
                    }
                    other => panic!("unexpected nested statement: {other:?}"),
                },
                other => panic!("unexpected then branch: {other:?}"),
            },
            other => panic!("unexpected first statement: {other:?}"),
        },
        other => panic!("unexpected always_ff body: {other:?}"),
    }
}

#[test]
fn parse_file_lowers_memory_declaration_and_read() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/overture/overture_fetch.sv"))
        .expect("parse overture_fetch");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.memories.len(), 1);
    assert_eq!(module.memories[0].name, "rom");
    assert_eq!(module.memories[0].element_width(), 8);
    assert_eq!(module.memories[0].depth(), 256);
    match &module.continuous_assignments[0].expr {
        Expr::MemoryRead { memory, .. } => assert_eq!(memory, "rom"),
        other => panic!("unexpected memory read expression: {other:?}"),
    }
}

#[test]
fn parse_file_lowers_always_ff_with_async_reset() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            "/virtual/top.sv",
            concat!(
                "module top(input logic clk, input logic reset, output logic q);",
                "always_ff @(posedge clk or posedge reset) begin ",
                "if (reset) q <= 1'b0; else q <= ~q; ",
                "end ",
                "endmodule\n"
            ),
        )
        .expect("parse async reset top");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(
        module.proc_blocks[0].kind,
        ProcBlockKind::AlwaysFf {
            clock: "clk".into(),
            async_reset: Some("reset".into()),
        }
    );
}

#[test]
fn parse_file_lowers_memory_element_write_in_always_ff() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/testing/memory_cpu_stub.sv"))
        .expect("parse memory_cpu_stub");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    match &module.proc_blocks[0].body {
        Stmt::Block(statements) => match &statements[0] {
            Stmt::If { else_branch, .. } => match else_branch.as_deref() {
                Some(Stmt::If { then_branch, .. }) => match then_branch.as_ref() {
                    Stmt::Block(statements) => {
                        let case_stmt = statements
                            .iter()
                            .find_map(|statement| match statement {
                                Stmt::Case { items, .. } => Some(items),
                                _ => None,
                            })
                            .expect("run branch should contain a case statement");
                        match &case_stmt[2].body {
                            Stmt::Assign {
                                kind,
                                target: LValue::MemoryElement { memory, .. },
                                ..
                            } => {
                                assert_eq!(*kind, AssignmentKind::Nonblocking);
                                assert_eq!(memory, "ram");
                            }
                            other => panic!("unexpected memory write statement: {other:?}"),
                        }
                    }
                    other => panic!("unexpected run branch body: {other:?}"),
                },
                other => panic!("unexpected else branch: {other:?}"),
            },
            other => panic!("unexpected first statement: {other:?}"),
        },
        other => panic!("unexpected always_ff body: {other:?}"),
    }
}

#[test]
fn parse_file_lowers_concatenation_assignments_and_shared_ansi_ports() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/testing/016-Vector3.sv"))
        .expect("parse 016-Vector3");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.ports.len(), 10);
    match &module.continuous_assignments[0].target {
        LValue::Concat(items) => assert_eq!(items.len(), 4),
        other => panic!("unexpected concatenation target: {other:?}"),
    }
    match &module.continuous_assignments[0].expr {
        Expr::Concat(items) => assert_eq!(items.len(), 7),
        other => panic!("unexpected concatenation expression: {other:?}"),
    }
}

#[test]
fn parse_file_lowers_replication_and_net_initializer() {
    let repo = repo_root();
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_file(&repo.join("parts/testing/019-Vector5.sv"))
        .expect("parse 019-Vector5");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.signals.len(), 2);
    assert_eq!(module.continuous_assignments.len(), 3);
    match &module.continuous_assignments[0].expr {
        Expr::Concat(items) => {
            assert_eq!(items.len(), 5);
            assert!(
                items
                    .iter()
                    .all(|item| matches!(item, Expr::Repeat { count: 5, .. }))
            );
        }
        other => panic!("unexpected replicated concatenation: {other:?}"),
    }
    match &module.continuous_assignments[1].expr {
        Expr::Repeat { count, expr } => {
            assert_eq!(*count, 5);
            assert!(matches!(expr.as_ref(), Expr::Concat(items) if items.len() == 5));
        }
        other => panic!("unexpected multiple concatenation: {other:?}"),
    }
}

#[test]
fn parse_str_preserves_storage_kinds_for_ports_signals_and_memories() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/storage_kinds.sv"),
            concat!(
                "module top(input wire a, output logic y);\n",
                "  wand pull_bus;\n",
                "  logic state;\n",
                "  logic [7:0] ram [0:3];\n",
                "  assign pull_bus = a;\n",
                "  assign y = pull_bus ^ state[0];\n",
                "endmodule\n",
            ),
        )
        .expect("parse storage kind module");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(
        module.port("a").expect("input port").storage,
        StorageKind::Net(NetKind::Wire)
    );
    assert_eq!(
        module.port("y").expect("output port").storage,
        StorageKind::Variable
    );
    assert_eq!(
        module.signal_decl("pull_bus").expect("net decl").storage,
        StorageKind::Net(NetKind::Wand)
    );
    assert_eq!(
        module.signal_decl("state").expect("variable decl").storage,
        StorageKind::Variable
    );
    assert_eq!(
        module.memory_decl("ram").expect("memory decl").storage,
        StorageKind::Variable
    );
}

#[test]
fn parse_str_prunes_constant_generate_else_if_chain() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/generate_top.sv"),
            r#"
module leaf_a(output logic y);
assign y = 1'b1;
endmodule

module leaf_b(output logic y);
assign y = 1'b0;
endmodule

module top #(parameter A = 0, parameter B = 1) (output logic y);
generate if (A) begin : gen_a
    leaf_a u_leaf(.y(y));
end else if (B) begin : gen_b
    leaf_b u_leaf(.y(y));
end else begin : gen_c
    assign y = 1'b1;
end endgenerate
endmodule
"#,
        )
        .expect("parse generated module");

    let module = source
        .modules
        .iter()
        .find(|module| module.name == "top")
        .expect("top module");
    assert!(module.unsupported.is_empty());
    assert_eq!(module.instantiations.len(), 1);
    assert_eq!(module.instantiations[0].module_name, "leaf_b");
    assert_eq!(module.instantiations[0].instance_name, "u_leaf");
    assert!(module.continuous_assignments.is_empty());
}

#[test]
fn parse_str_prunes_generate_for_negated_localparam_condition() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/negated_generate.sv"),
            r#"
module top(output logic y);
localparam NEG = -1;
generate if (NEG) begin : gen_true
    assign y = 1'b1;
end else begin : gen_false
    assign y = 1'b0;
end endgenerate
endmodule
"#,
        )
        .expect("parse negated generate");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.continuous_assignments.len(), 1);
    match &module.continuous_assignments[0].expr {
        Expr::Literal(NumericLiteral { bits, .. }) => {
            assert_eq!(
                bits.to_bit_value_checked()
                    .and_then(|bits| bits.to_u64_checked()),
                Some(1)
            );
        }
        other => panic!("unexpected generated assignment: {other:?}"),
    }
}

#[test]
fn parse_str_lowers_signedness_casts_in_constant_parameter_expressions() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/constant_signedness.sv"),
            r#"
module top(output logic y);
localparam SIGNED_LT = $signed(8'hff) < $signed(1'b0);
localparam UNSIGNED_EQ = $unsigned($signed(8'hff)) == 8'hff;
generate if (SIGNED_LT && UNSIGNED_EQ) begin : gen_true
    assign y = 1'b1;
end else begin : gen_false
    assign y = 1'b0;
end endgenerate
endmodule
"#,
        )
        .expect("parse constant signedness generate");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.parameters.len(), 2);
    assert_eq!(module.continuous_assignments.len(), 1);
    match &module.continuous_assignments[0].expr {
        Expr::Literal(NumericLiteral { bits, .. }) => {
            assert_eq!(
                bits.to_bit_value_checked()
                    .and_then(|bits| bits.to_u64_checked()),
                Some(1)
            );
        }
        other => panic!("unexpected generated assignment: {other:?}"),
    }
}

#[test]
fn parse_str_unrolls_procedural_for_loops_with_constant_indexed_part_selects() {
    fn collect_assignments<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Stmt>) {
        match stmt {
            Stmt::Assign { .. } => out.push(stmt),
            Stmt::Block(statements) => {
                for statement in statements {
                    collect_assignments(statement, out);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_assignments(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_assignments(else_branch, out);
                }
            }
            Stmt::Case { items, default, .. } => {
                for item in items {
                    collect_assignments(&item.body, out);
                }
                if let Some(default) = default {
                    collect_assignments(default, out);
                }
            }
            Stmt::Empty => {}
        }
    }

    fn expr_contains_ident(expr: &Expr, ident: &str) -> bool {
        match expr {
            Expr::Ident(name) => name == ident,
            Expr::Literal(_) => false,
            Expr::Concat(items) => items.iter().any(|item| expr_contains_ident(item, ident)),
            Expr::Repeat { expr, .. } => expr_contains_ident(expr, ident),
            Expr::MemoryRead { index, .. } => expr_contains_ident(index, ident),
            Expr::BitSelect { expr, .. } => expr_contains_ident(expr, ident),
            Expr::PartSelect { expr, .. } => expr_contains_ident(expr, ident),
            Expr::Unary { expr, .. } => expr_contains_ident(expr, ident),
            Expr::Binary { left, right, .. } => {
                expr_contains_ident(left, ident) || expr_contains_ident(right, ident)
            }
            Expr::Ternary {
                cond,
                when_true,
                when_false,
            } => {
                expr_contains_ident(cond, ident)
                    || expr_contains_ident(when_true, ident)
                    || expr_contains_ident(when_false, ident)
            }
        }
    }

    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/procedural_for.sv"),
            r#"
module top #(parameter STRIDE = 2) (
input logic [7:0] in,
output logic [7:0] out
);
integer i, j;

always @* begin
    out = 8'h00;
    for (i = 0; i < 2; i = i + 1) begin
        for (j = 0; j < 4; j = j + STRIDE)
            out[j + i * 4 +: STRIDE] = in[j + i * 4 +: STRIDE] + i;
    end
end
endmodule
"#,
        )
        .expect("parse procedural for module");

    let module = &source.modules[0];
    assert!(
        module.unsupported.is_empty(),
        "unexpected unsupported entries: {:?}",
        module.unsupported
    );
    assert_eq!(module.proc_blocks.len(), 1);

    let mut assignments = Vec::new();
    collect_assignments(&module.proc_blocks[0].body, &mut assignments);
    assert_eq!(assignments.len(), 5);

    let mut actual_ranges = Vec::new();
    let mut actual_increments = Vec::new();
    for assignment in &assignments[1..] {
        let Stmt::Assign { target, expr, .. } = assignment else {
            panic!("expected assignment");
        };
        let LValue::PartSelect { signal, msb, lsb } = target else {
            panic!("expected constant part-select target: {assignment:?}");
        };
        assert_eq!(signal, "out");
        actual_ranges.push((*msb, *lsb));
        assert!(
            !expr_contains_ident(expr, "i") && !expr_contains_ident(expr, "j"),
            "loop variables should be substituted away: {expr:?}"
        );
        let Expr::Binary { right, .. } = expr else {
            panic!("expected binary add expression: {expr:?}");
        };
        match right.as_ref() {
            Expr::Literal(NumericLiteral { bits, .. }) => {
                let value = bits
                    .to_bit_value_checked()
                    .and_then(|bits| bits.to_u64_checked())
                    .expect("literal increment");
                actual_increments.push(value);
            }
            Expr::Unary { op, expr } if *op == crate::hir::UnaryOp::Signed => match expr.as_ref() {
                Expr::Literal(NumericLiteral { bits, .. }) => {
                    let value = bits
                        .to_bit_value_checked()
                        .and_then(|bits| bits.to_u64_checked())
                        .expect("signed increment");
                    actual_increments.push(value);
                }
                other => panic!("unexpected signed increment expression: {other:?}"),
            },
            other => panic!("unexpected increment expression: {other:?}"),
        }
    }

    assert_eq!(actual_ranges, vec![(1, 0), (3, 2), (5, 4), (7, 6)]);
    assert_eq!(actual_increments, vec![0, 0, 1, 1]);
}

#[test]
fn parse_str_preserves_comparison_operands_across_logical_and_rebalancing() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/precedence_if.sv"),
            r#"
module top(
input logic [1:0] mem_wordsize,
input logic [31:0] reg_op1,
output logic trapit
);
always_comb begin
    trapit = 1'b0;
    if (mem_wordsize == 0 && reg_op1[1:0] != 0)
        trapit = 1'b1;
end
endmodule
"#,
        )
        .expect("parse precedence_if");

    let module = &source.modules[0];
    let Stmt::Block(statements) = &module.proc_blocks[0].body else {
        panic!("expected always_comb block");
    };
    let Stmt::If { cond, .. } = &statements[1] else {
        panic!("expected conditional statement");
    };
    match cond {
        Expr::Binary {
            left,
            op: BinaryOp::LogicalAnd,
            right,
        } => {
            assert!(matches!(
                left.as_ref(),
                Expr::Binary {
                    op: BinaryOp::Eq,
                    ..
                }
            ));
            assert!(matches!(
                right.as_ref(),
                Expr::Binary {
                    left: _,
                    op: BinaryOp::NotEq,
                    right: _
                }
            ));
        }
        other => panic!("unexpected lowered condition: {other:?}"),
    }
}

#[test]
fn parse_str_short_circuits_const_false_logical_and_during_pruning() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/short_circuit_prune.sv"),
            r#"
module top(
input logic irq_pending,
output logic seen
);
localparam ENABLE_IRQ = 1'b0;
logic [7:0] next_irq_pending;
logic irq_active;
integer irq_buserror;

always_comb begin
    seen = 1'b0;
    if (ENABLE_IRQ && irq_pending && !irq_active) begin
        next_irq_pending[irq_buserror] = 1'b1;
        seen = 1'b1;
    end
end
endmodule
"#,
        )
        .expect("parse short-circuit prune");

    let module = &source.modules[0];
    assert!(
        module.unsupported.is_empty(),
        "dead constant-false branch should be pruned before unsupported lowering: {:?}",
        module.unsupported
    );

    let Stmt::Block(statements) = &module.proc_blocks[0].body else {
        panic!("expected always_comb block");
    };
    assert_eq!(statements.len(), 2);
    assert!(matches!(statements[1], Stmt::Empty));
}

#[test]
fn parse_str_treats_inert_debug_constructs_as_empty_statements() {
    let frontend = SvParserFrontend::default();
    let source = frontend
        .parse_str(
            PathBuf::from("/virtual/design/inert_debug.sv"),
            r#"
module top(input logic a, output logic y);
task empty_statement;
    begin end
endtask

always @* begin
    y = 1'b0;
    empty_statement;
    $display("debug");
    (* parallel_case *)
    case (1'b1)
        a: y = 1'b1;
        default: y = 1'b0;
    endcase
end
endmodule
"#,
        )
        .expect("parse inert debug module");

    let module = &source.modules[0];
    assert!(module.unsupported.is_empty());
    assert_eq!(module.proc_blocks.len(), 1);
    match &module.proc_blocks[0].body {
        Stmt::Block(statements) => {
            assert!(matches!(statements[0], Stmt::Assign { .. }));
            assert!(matches!(statements[1], Stmt::Empty));
            assert!(matches!(statements[2], Stmt::Empty));
            assert!(matches!(statements[3], Stmt::Case { .. }));
        }
        other => panic!("unexpected always body: {other:?}"),
    }
}
