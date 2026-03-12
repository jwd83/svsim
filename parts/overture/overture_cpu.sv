module overture_cpu (
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

    // Architectural registers
    reg [7:0] r0;
    reg [7:0] r1;
    reg [7:0] r2;
    reg [7:0] r3;
    reg [7:0] r4;
    reg [7:0] r5;

    // --- Fetch: ROM lookup by PC ---
    wire [7:0] instr;

    overture_fetch fetch_unit (
        .addr(pc),
        .data(instr)
    );

    // --- Decode: extract instruction fields ---
    wire       is_immediate;
    wire       is_calculate;
    wire       is_copy;
    wire       is_condition;
    wire [5:0] imm_value;
    wire [2:0] alu_op;
    wire [2:0] src_sel;
    wire [2:0] dst_sel;
    wire [2:0] cond_sel;

    overture_decoder_8bit decoder (
        .instr(instr),
        .is_immediate(is_immediate),
        .is_calculate(is_calculate),
        .is_copy(is_copy),
        .is_condition(is_condition),
        .imm_value(imm_value),
        .alu_op(alu_op),
        .src_sel(src_sel),
        .dst_sel(dst_sel),
        .cond_sel(cond_sel)
    );

    // --- Execute: ALU, source mux, condition evaluator ---
    wire [7:0] alu_result;

    overture_alu_8bit alu (
        .inA(r1),
        .inB(r2),
        .op(alu_op),
        .outY(alu_result)
    );

    wire [7:0] src_value;

    mux_8to1_8bit src_mux (
        .in0(r0),
        .in1(r1),
        .in2(r2),
        .in3(r3),
        .in4(r4),
        .in5(r5),
        .in6(in_port),
        .in7(8'b0),
        .sel(src_sel),
        .out(src_value)
    );

    wire cond_met;

    overture_condition cond_eval (
        .r3(r3),
        .cond_sel(cond_sel),
        .cond_met(cond_met)
    );

    // --- Jump logic ---
    wire jump_en;
    assign jump_en = is_condition & cond_met;

    // --- Register writeback and PC update ---
    always_ff @(posedge clk)
        if (reset) begin
            pc          <= 8'b0;
            out_port    <= 8'b0;
            instr_debug <= 8'b0;
            r0          <= 8'b0;
            r1          <= 8'b0;
            r2          <= 8'b0;
            r3          <= 8'b0;
            r4          <= 8'b0;
            r5          <= 8'b0;
        end else if (run) begin
            instr_debug <= instr;

            if (jump_en) begin
                pc <= r0;
            end else begin
                pc <= pc + 1'b1;
            end

            if (is_immediate)
                r0 <= imm_value;

            if (is_calculate)
                r3 <= alu_result;

            if (is_copy)
                case (dst_sel)
                    3'b000: r0 <= src_value;
                    3'b001: r1 <= src_value;
                    3'b010: r2 <= src_value;
                    3'b011: r3 <= src_value;
                    3'b100: r4 <= src_value;
                    3'b101: r5 <= src_value;
                    3'b110: out_port <= src_value;
                endcase
        end

    // Debug / observation outputs
    assign r0_out = r0;
    assign r1_out = r1;
    assign r2_out = r2;
    assign r3_out = r3;
    assign r4_out = r4;
    assign r5_out = r5;

endmodule
