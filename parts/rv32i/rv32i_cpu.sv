module rv32i_cpu (
    input         clk,
    input         reset,
    input         run,
    output [31:0] pc,
    output [31:0] instr_debug,
    output        halted,
    output [31:0] x1_out,
    output [31:0] x2_out,
    output [31:0] x3_out,
    output [31:0] x4_out,
    output [31:0] x5_out,
    output [31:0] mem0_out,
    output [31:0] mem1_out,
    output [31:0] mem2_out
);

    // Minimal RV32I demo core:
    // - Byte-addressed 32-bit PC
    // - Word-aligned LW/SW into an internal RAM
    // - Real RV32I encodings for ADDI/ADD/SUB/AND/OR/XOR/LUI/LW/SW/BEQ/BNE/JAL
    // - Treats `jal x0, 0` as a demo halt instruction

    reg [31:0] regs [31:0];
    reg [31:0] imem [63:0];
    reg [31:0] dmem [63:0];

    reg        reg_write_en;
    reg        store_en;
    reg [31:0] next_pc;
    reg [31:0] rd_write_value;

    wire [31:0] instr;
    assign instr = imem[pc[7:2]];

    wire [6:0] opcode = instr[6:0];
    wire [2:0] funct3 = instr[14:12];
    wire [6:0] funct7 = instr[31:25];
    wire [4:0] rs1_idx = instr[19:15];
    wire [4:0] rs2_idx = instr[24:20];
    wire [4:0] rd_idx = instr[11:7];

    wire [31:0] rs1_value = regs[rs1_idx];
    wire [31:0] rs2_value = regs[rs2_idx];

    wire [31:0] imm_i = {{20{instr[31]}}, instr[31:20]};
    wire [31:0] imm_s = {{20{instr[31]}}, instr[31:25], instr[11:7]};
    wire [31:0] imm_b = {{19{instr[31]}}, instr[31], instr[7], instr[30:25], instr[11:8], 1'b0};
    wire [31:0] imm_u = {instr[31:12], 12'b0};
    wire [31:0] imm_j = {{11{instr[31]}}, instr[31], instr[19:12], instr[20], instr[30:21], 1'b0};

    wire is_op_imm = opcode == 7'b0010011;
    wire is_op     = opcode == 7'b0110011;
    wire is_load   = opcode == 7'b0000011;
    wire is_store  = opcode == 7'b0100011;
    wire is_branch = opcode == 7'b1100011;
    wire is_lui    = opcode == 7'b0110111;
    wire is_jal    = opcode == 7'b1101111;

    wire is_addi = is_op_imm & (funct3 == 3'b000);
    wire is_xori = is_op_imm & (funct3 == 3'b100);
    wire is_ori  = is_op_imm & (funct3 == 3'b110);
    wire is_andi = is_op_imm & (funct3 == 3'b111);

    wire is_add = is_op & (funct3 == 3'b000) & (funct7 == 7'b0000000);
    wire is_sub = is_op & (funct3 == 3'b000) & (funct7 == 7'b0100000);
    wire is_xor = is_op & (funct3 == 3'b100) & (funct7 == 7'b0000000);
    wire is_or  = is_op & (funct3 == 3'b110) & (funct7 == 7'b0000000);
    wire is_and = is_op & (funct3 == 3'b111) & (funct7 == 7'b0000000);

    wire is_lw  = is_load & (funct3 == 3'b010);
    wire is_sw  = is_store & (funct3 == 3'b010);
    wire is_beq = is_branch & (funct3 == 3'b000);
    wire is_bne = is_branch & (funct3 == 3'b001);

    wire [31:0] data_addr = rs1_value + (is_sw ? imm_s : imm_i);
    wire [5:0] data_word_index = data_addr[7:2];
    wire [31:0] load_word = dmem[data_word_index];

    wire rs_equal = rs1_value == rs2_value;
    wire branch_taken = (is_beq & rs_equal) | (is_bne & (rs_equal == 1'b0));
    wire is_halt = instr == 32'h0000006f;

    always_comb begin
        reg_write_en = 1'b0;
        store_en = 1'b0;
        next_pc = pc + 32'd4;
        rd_write_value = 32'b0;

        if (is_halt) begin
            next_pc = pc;
        end else if (branch_taken) begin
            next_pc = pc + imm_b;
        end else if (is_jal) begin
            reg_write_en = 1'b1;
            rd_write_value = pc + 32'd4;
            next_pc = pc + imm_j;
        end else if (is_lui) begin
            reg_write_en = 1'b1;
            rd_write_value = imm_u;
        end else if (is_addi) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value + imm_i;
        end else if (is_xori) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value ^ imm_i;
        end else if (is_ori) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value | imm_i;
        end else if (is_andi) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value & imm_i;
        end else if (is_add) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value + rs2_value;
        end else if (is_sub) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value - rs2_value;
        end else if (is_xor) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value ^ rs2_value;
        end else if (is_or) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value | rs2_value;
        end else if (is_and) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value & rs2_value;
        end else if (is_lw) begin
            reg_write_en = 1'b1;
            rd_write_value = load_word;
        end else if (is_sw) begin
            store_en = 1'b1;
        end
    end

    always_ff @(posedge clk) begin
        if (reset) begin
            pc <= 32'b0;
            instr_debug <= 32'b0;
            halted <= 1'b0;
            regs[0] <= 32'b0;
        end else if (run && (halted == 1'b0)) begin
            instr_debug <= instr;

            if (is_halt) begin
                halted <= 1'b1;
            end else begin
                pc <= next_pc;

                if (reg_write_en && (rd_idx != 5'b00000))
                    regs[rd_idx] <= rd_write_value;

                if (store_en)
                    dmem[data_word_index] <= rs2_value;
            end

            regs[0] <= 32'b0;
        end
    end

    assign x1_out = regs[1];
    assign x2_out = regs[2];
    assign x3_out = regs[3];
    assign x4_out = regs[4];
    assign x5_out = regs[5];
    assign mem0_out = dmem[0];
    assign mem1_out = dmem[1];
    assign mem2_out = dmem[2];

endmodule
