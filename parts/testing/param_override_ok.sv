// Instance parameter overrides the frozen-parameter fence must allow:
// runtime-only parameters may take any value, and frozen parameters may be
// re-stated at their lowering-time default.
module override_add_leaf #(
    parameter [7:0] OFFSET = 8'd1
) (
    input  [7:0] a,
    output [7:0] y
);
    assign y = a + OFFSET;
endmodule

module override_width_leaf #(
    parameter WIDTH = 8
) (
    input  [WIDTH-1:0] a,
    output [WIDTH-1:0] y
);
    assign y = ~a;
endmodule

module param_override_ok (
    input  [7:0] a,
    output [7:0] sum,
    output [7:0] inv
);
    override_add_leaf #(.OFFSET(8'd7)) u_add (.a(a), .y(sum));
    override_width_leaf #(.WIDTH(8)) u_inv (.a(a), .y(inv));
endmodule
