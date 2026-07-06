// Negative case: N is frozen into loop_leaf's unrolled `for` loop at lowering
// time, so a non-default instance override must be rejected at elaboration.
module loop_leaf #(
    parameter N = 2
) (
    input  [3:0] data,
    output logic [3:0] s
);
    integer i;
    always_comb begin
        s = 4'b0000;
        for (i = 0; i < N; i = i + 1) begin
            s = s ^ data;
        end
    end
endmodule

module param_override_frozen_loop (
    input  [3:0] data,
    output [3:0] s
);
    loop_leaf #(.N(3)) u_leaf (.data(data), .s(s));
endmodule
