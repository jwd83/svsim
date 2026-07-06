// Negative case: WIDTH is frozen into frozen_leaf's port ranges at lowering
// time, so a non-default instance override must be rejected at elaboration.
module frozen_leaf #(
    parameter WIDTH = 8
) (
    input  [WIDTH-1:0] a,
    output [WIDTH-1:0] y
);
    assign y = ~a;
endmodule

module param_override_frozen_range (
    input  [7:0] a,
    output [7:0] y
);
    frozen_leaf #(.WIDTH(4)) u_leaf (.a(a), .y(y));
endmodule
