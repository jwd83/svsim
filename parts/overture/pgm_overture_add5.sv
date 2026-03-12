module pgm_overture_add5 (
    input        clk,
    input        reset,
    input        run,
    input  [7:0] in_port,
    output [7:0] pc,
    output [7:0] out_port,
    output [7:0] instr_debug,
    output [7:0] r0_out,
    output [7:0] r1_out,
    output [7:0] r2_out,
    output [7:0] r3_out,
    output [7:0] r4_out,
    output [7:0] r5_out
);

    // Program wrapper for overture_add5.txt.
    overture_cpu cpu (
        .clk(clk),
        .reset(reset),
        .run(run),
        .in_port(in_port),
        .pc(pc),
        .out_port(out_port),
        .instr_debug(instr_debug),
        .r0_out(r0_out),
        .r1_out(r1_out),
        .r2_out(r2_out),
        .r3_out(r3_out),
        .r4_out(r4_out),
        .r5_out(r5_out)
    );

endmodule
