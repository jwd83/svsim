module parameter_defaults #(
    parameter [7:0] OFFSET = 8'd10,
    parameter [7:0] MASK   = 8'hF0
) (
    input  [7:0] a,
    output [7:0] y
);
    assign y = (a + OFFSET) & MASK;
endmodule
