// ===========================================================
// 8x8-bit Register File with Dual Read Ports
// ===========================================================
// Write: write_en, write_addr[2:0], write_data[7:0]
// Read:  read_addr1[2:0] -> read_data1[7:0]
//        read_addr2[2:0] -> read_data2[7:0]
//
// 8 registers of 8 bits each, one write port, two read ports.

module regfile_8x8 (
    input  logic       clk,
    input  logic       write_en,
    input  logic [2:0] write_addr,
    input  logic [7:0] write_data,
    input  logic [2:0] read_addr1,
    input  logic [2:0] read_addr2,
    output logic [7:0] read_data1,
    output logic [7:0] read_data2
);
    // Decode write address to one-hot
    logic [7:0] dec_out;
    decoder_3to8 u_dec (.addr(write_addr), .out(dec_out));

    // Gate each decoder output with write_en
    logic wen0, wen1, wen2, wen3, wen4, wen5, wen6, wen7;
    and_gate u_wen0 (.inA(write_en), .inB(dec_out[0]), .outY(wen0));
    and_gate u_wen1 (.inA(write_en), .inB(dec_out[1]), .outY(wen1));
    and_gate u_wen2 (.inA(write_en), .inB(dec_out[2]), .outY(wen2));
    and_gate u_wen3 (.inA(write_en), .inB(dec_out[3]), .outY(wen3));
    and_gate u_wen4 (.inA(write_en), .inB(dec_out[4]), .outY(wen4));
    and_gate u_wen5 (.inA(write_en), .inB(dec_out[5]), .outY(wen5));
    and_gate u_wen6 (.inA(write_en), .inB(dec_out[6]), .outY(wen6));
    and_gate u_wen7 (.inA(write_en), .inB(dec_out[7]), .outY(wen7));

    // 8 registers
    logic [7:0] r0, r1, r2, r3, r4, r5, r6, r7;
    register_8bit u_reg0 (.clk(clk), .enable(wen0), .data(write_data), .q(r0));
    register_8bit u_reg1 (.clk(clk), .enable(wen1), .data(write_data), .q(r1));
    register_8bit u_reg2 (.clk(clk), .enable(wen2), .data(write_data), .q(r2));
    register_8bit u_reg3 (.clk(clk), .enable(wen3), .data(write_data), .q(r3));
    register_8bit u_reg4 (.clk(clk), .enable(wen4), .data(write_data), .q(r4));
    register_8bit u_reg5 (.clk(clk), .enable(wen5), .data(write_data), .q(r5));
    register_8bit u_reg6 (.clk(clk), .enable(wen6), .data(write_data), .q(r6));
    register_8bit u_reg7 (.clk(clk), .enable(wen7), .data(write_data), .q(r7));

    // Read port 1
    mux_8to1_8bit u_rmux1 (
        .in0(r0), .in1(r1), .in2(r2), .in3(r3),
        .in4(r4), .in5(r5), .in6(r6), .in7(r7),
        .sel(read_addr1), .out(read_data1)
    );

    // Read port 2
    mux_8to1_8bit u_rmux2 (
        .in0(r0), .in1(r1), .in2(r2), .in3(r3),
        .in4(r4), .in5(r5), .in6(r6), .in7(r7),
        .sel(read_addr2), .out(read_data2)
    );

endmodule
