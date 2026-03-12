// ===========================================================
// 2:1 Multiplexer (16-bit)
// ===========================================================
// Inputs: in0[15:0], in1[15:0], sel
// Output: out[15:0]
//
// Selects between two 16-bit inputs based on select signal
// sel = 0: out = in0
// sel = 1: out = in1

module mux_2to1_16bit (
    input  logic [15:0] in0,   // 16-bit Input 0
    input  logic [15:0] in1,   // 16-bit Input 1
    input  logic        sel,   // Select signal
    output logic [15:0] out    // 16-bit Output
);
    // Use sixteen 1-bit multiplexers for each bit position
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

endmodule