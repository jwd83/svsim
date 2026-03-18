module param_cross_ref #(
    parameter [7:0] BASE = 8'd5
) (
    input  [7:0] a,
    output [7:0] y
);
    localparam [7:0] DOUBLED = BASE + BASE;

    assign y = a + DOUBLED;
endmodule
