module ternary_mux (
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic       sel,
    output logic [7:0] out
);

    assign out = sel ? a : b;

endmodule
