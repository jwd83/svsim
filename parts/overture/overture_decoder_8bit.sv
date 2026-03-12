module overture_decoder_8bit (
    input  [7:0] instr,
    output       is_immediate,
    output       is_calculate,
    output       is_copy,
    output       is_condition,
    output [5:0] imm_value,
    output [2:0] alu_op,
    output [2:0] src_sel,
    output [2:0] dst_sel,
    output [2:0] cond_sel
);

    assign is_immediate = (instr[7:6] == 2'b00);
    assign is_calculate = (instr[7:6] == 2'b01);
    assign is_copy      = (instr[7:6] == 2'b10);
    assign is_condition = (instr[7:6] == 2'b11);

    assign imm_value = instr[5:0];
    assign alu_op    = instr[2:0];
    assign src_sel   = instr[5:3];
    assign dst_sel   = instr[2:0];
    assign cond_sel  = instr[2:0];

endmodule
