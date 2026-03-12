// ===========================================================
// 2:1 Multiplexer (32-bit)
// ===========================================================
// Inputs: in0[31:0], in1[31:0], sel
// Output: out[31:0]
//
// Selects between two 32-bit inputs based on select signal
// sel = 0: out = in0
// sel = 1: out = in1

module mux_2to1_32bit (
    input  logic [31:0] in0,   // 32-bit Input 0
    input  logic [31:0] in1,   // 32-bit Input 1
    input  logic        sel,   // Select signal
    output logic [31:0] out    // 32-bit Output
);
    // Use thirty-two 1-bit multiplexers for each bit position
    mux_2to1_1bit u_mux0  (.in0(in0[0]),  .in1(in1[0]),  .sel(sel), .out(out[0]));
    mux_2to1_1bit u_mux1  (.in0(in0[1]),  .in1(in1[1]),  .sel(sel), .out(out[1]));
    mux_2to1_1bit u_mux2  (.in0(in0[2]),  .in1(in1[2]),  .sel(sel), .out(out[2]));
    mux_2to1_1bit u_mux3  (.in0(in0[3]),  .in1(in1[3]),  .sel(sel), .out(out[3]));
    mux_2to1_1bit u_mux4  (.in0(in0[4]),  .in1(in1[4]),  .sel(sel), .out(out[4]));
    mux_2to1_1bit u_mux5  (.in0(in0[5]),  .in1(in1[5]),  .sel(sel), .out(out[5]));
    mux_2to1_1bit u_mux6  (.in0(in0[6]),  .in1(in1[6]),  .sel(sel), .out(out[6]));
    mux_2to1_1bit u_mux7  (.in0(in0[7]),  .in1(in1[7]),  .sel(sel), .out(out[7]));
    mux_2to1_1bit u_mux8  (.in0(in0[8]),  .in1(in1[8]),  .sel(sel), .out(out[8]));
    mux_2to1_1bit u_mux9  (.in0(in0[9]),  .in1(in1[9]),  .sel(sel), .out(out[9]));
    mux_2to1_1bit u_mux10 (.in0(in0[10]), .in1(in1[10]), .sel(sel), .out(out[10]));
    mux_2to1_1bit u_mux11 (.in0(in0[11]), .in1(in1[11]), .sel(sel), .out(out[11]));
    mux_2to1_1bit u_mux12 (.in0(in0[12]), .in1(in1[12]), .sel(sel), .out(out[12]));
    mux_2to1_1bit u_mux13 (.in0(in0[13]), .in1(in1[13]), .sel(sel), .out(out[13]));
    mux_2to1_1bit u_mux14 (.in0(in0[14]), .in1(in1[14]), .sel(sel), .out(out[14]));
    mux_2to1_1bit u_mux15 (.in0(in0[15]), .in1(in1[15]), .sel(sel), .out(out[15]));
    mux_2to1_1bit u_mux16 (.in0(in0[16]), .in1(in1[16]), .sel(sel), .out(out[16]));
    mux_2to1_1bit u_mux17 (.in0(in0[17]), .in1(in1[17]), .sel(sel), .out(out[17]));
    mux_2to1_1bit u_mux18 (.in0(in0[18]), .in1(in1[18]), .sel(sel), .out(out[18]));
    mux_2to1_1bit u_mux19 (.in0(in0[19]), .in1(in1[19]), .sel(sel), .out(out[19]));
    mux_2to1_1bit u_mux20 (.in0(in0[20]), .in1(in1[20]), .sel(sel), .out(out[20]));
    mux_2to1_1bit u_mux21 (.in0(in0[21]), .in1(in1[21]), .sel(sel), .out(out[21]));
    mux_2to1_1bit u_mux22 (.in0(in0[22]), .in1(in1[22]), .sel(sel), .out(out[22]));
    mux_2to1_1bit u_mux23 (.in0(in0[23]), .in1(in1[23]), .sel(sel), .out(out[23]));
    mux_2to1_1bit u_mux24 (.in0(in0[24]), .in1(in1[24]), .sel(sel), .out(out[24]));
    mux_2to1_1bit u_mux25 (.in0(in0[25]), .in1(in1[25]), .sel(sel), .out(out[25]));
    mux_2to1_1bit u_mux26 (.in0(in0[26]), .in1(in1[26]), .sel(sel), .out(out[26]));
    mux_2to1_1bit u_mux27 (.in0(in0[27]), .in1(in1[27]), .sel(sel), .out(out[27]));
    mux_2to1_1bit u_mux28 (.in0(in0[28]), .in1(in1[28]), .sel(sel), .out(out[28]));
    mux_2to1_1bit u_mux29 (.in0(in0[29]), .in1(in1[29]), .sel(sel), .out(out[29]));
    mux_2to1_1bit u_mux30 (.in0(in0[30]), .in1(in1[30]), .sel(sel), .out(out[30]));
    mux_2to1_1bit u_mux31 (.in0(in0[31]), .in1(in1[31]), .sel(sel), .out(out[31]));

endmodule