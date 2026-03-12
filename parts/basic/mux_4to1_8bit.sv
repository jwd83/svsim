// ===========================================================
// 4:1 Multiplexer (8-bit)
// ===========================================================
// Inputs: in0[7:0], in1[7:0], in2[7:0], in3[7:0], sel[1:0]
// Output: out[7:0]
//
// sel=0: out=in0, sel=1: out=in1, sel=2: out=in2, sel=3: out=in3

module mux_4to1_8bit (
    input  logic [7:0] in0,
    input  logic [7:0] in1,
    input  logic [7:0] in2,
    input  logic [7:0] in3,
    input  logic [1:0] sel,
    output logic [7:0] out
);
    // First stage: select between pairs using sel[0]
    logic [7:0] mux_lo, mux_hi;

    mux_2to1_8bit u_mux_lo (.in0(in0), .in1(in1), .sel(sel[0]), .out(mux_lo));
    mux_2to1_8bit u_mux_hi (.in0(in2), .in1(in3), .sel(sel[0]), .out(mux_hi));

    // Second stage: select between results using sel[1]
    mux_2to1_8bit u_mux_out (.in0(mux_lo), .in1(mux_hi), .sel(sel[1]), .out(out));

endmodule
