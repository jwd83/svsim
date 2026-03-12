// ===========================================================
// 2:1 Multiplexer (4-bit)
// ===========================================================
// Inputs: in0[3:0], in1[3:0], sel
// Output: out[3:0]
//
// Selects between two 4-bit inputs based on select signal
// sel = 0: out = in0
// sel = 1: out = in1

module mux_2to1_4bit (
    input  logic [3:0] in0,   // 4-bit Input 0
    input  logic [3:0] in1,   // 4-bit Input 1
    input  logic       sel,   // Select signal
    output logic [3:0] out    // 4-bit Output
);
    // Use four 1-bit multiplexers for each bit position
    mux_2to1_1bit u_mux0 (.in0(in0[0]), .in1(in1[0]), .sel(sel), .out(out[0]));
    mux_2to1_1bit u_mux1 (.in0(in0[1]), .in1(in1[1]), .sel(sel), .out(out[1]));
    mux_2to1_1bit u_mux2 (.in0(in0[2]), .in1(in1[2]), .sel(sel), .out(out[2]));
    mux_2to1_1bit u_mux3 (.in0(in0[3]), .in1(in1[3]), .sel(sel), .out(out[3]));

endmodule