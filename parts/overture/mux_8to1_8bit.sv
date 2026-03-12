// ===========================================================
// 8:1 Multiplexer (8-bit)
// ===========================================================
// Inputs: in0..in7[7:0], sel[2:0]
// Output: out[7:0]
//
// sel=0: out=in0, sel=1: out=in1, ... sel=7: out=in7

module mux_8to1_8bit (
    input  logic [7:0] in0,
    input  logic [7:0] in1,
    input  logic [7:0] in2,
    input  logic [7:0] in3,
    input  logic [7:0] in4,
    input  logic [7:0] in5,
    input  logic [7:0] in6,
    input  logic [7:0] in7,
    input  logic [2:0] sel,
    output logic [7:0] out
);
    // First stage: two 4:1 muxes using sel[1:0]
    logic [7:0] mux_lo, mux_hi;

    mux_4to1_8bit u_mux_lo (
        .in0(in0), .in1(in1), .in2(in2), .in3(in3),
        .sel(sel[1:0]), .out(mux_lo)
    );

    mux_4to1_8bit u_mux_hi (
        .in0(in4), .in1(in5), .in2(in6), .in3(in7),
        .sel(sel[1:0]), .out(mux_hi)
    );

    // Second stage: select between halves using sel[2]
    mux_2to1_8bit u_mux_out (.in0(mux_lo), .in1(mux_hi), .sel(sel[2]), .out(out));

endmodule
