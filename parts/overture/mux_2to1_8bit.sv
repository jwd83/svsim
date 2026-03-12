// ===========================================================
// 2:1 Multiplexer (8-bit)
// ===========================================================
// Inputs: in0[7:0], in1[7:0], sel
// Output: out[7:0]
//
// Selects between two 8-bit inputs based on select signal
// sel = 0: out = in0
// sel = 1: out = in1

module mux_2to1_8bit (
    input  logic [7:0] in0,   // 8-bit Input 0
    input  logic [7:0] in1,   // 8-bit Input 1
    input  logic       sel,   // Select signal
    output logic [7:0] out    // 8-bit Output
);
    // Use eight 1-bit multiplexers for each bit position
    mux_2to1_1bit u_mux0 (.in0(in0[0]), .in1(in1[0]), .sel(sel), .out(out[0]));
    mux_2to1_1bit u_mux1 (.in0(in0[1]), .in1(in1[1]), .sel(sel), .out(out[1]));
    mux_2to1_1bit u_mux2 (.in0(in0[2]), .in1(in1[2]), .sel(sel), .out(out[2]));
    mux_2to1_1bit u_mux3 (.in0(in0[3]), .in1(in1[3]), .sel(sel), .out(out[3]));
    mux_2to1_1bit u_mux4 (.in0(in0[4]), .in1(in1[4]), .sel(sel), .out(out[4]));
    mux_2to1_1bit u_mux5 (.in0(in0[5]), .in1(in1[5]), .sel(sel), .out(out[5]));
    mux_2to1_1bit u_mux6 (.in0(in0[6]), .in1(in1[6]), .sel(sel), .out(out[6]));
    mux_2to1_1bit u_mux7 (.in0(in0[7]), .in1(in1[7]), .sel(sel), .out(out[7]));

endmodule