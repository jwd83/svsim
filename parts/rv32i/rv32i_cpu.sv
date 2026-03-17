module rv32i_cpu (
    input         clk,
    input         reset,
    input         run,
    output [31:0] pc,
    output [31:0] instr_debug,
    output        halted,
    output        trap,
    output [31:0] trap_cause,
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
    // - 32-bit internal RAM with byte and halfword lane selection inside each word
    // - Misaligned halfword/word data accesses raise simple load/store traps
    // - Real RV32I encodings for arithmetic, logical, compare, branch, jump, load/store, fence, and basic system ops
    // - Treats `jal x0, 0` as a demo halt instruction
    // - Surfaces `ecall`, `ebreak`, and unrecognized instructions as simple traps

    reg [31:0] regs [31:0];
    reg [31:0] imem [63:0];
    reg [31:0] dmem [63:0];

    reg        reg_write_en;
    reg        store_en;
    reg [31:0] next_pc;
    reg [31:0] rd_write_value;
    reg [31:0] store_write_value;

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
    wire [4:0] shamt_i = instr[24:20];
    wire [4:0] shamt_r = rs2_value[4:0];

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
    wire is_auipc  = opcode == 7'b0010111;
    wire is_jal    = opcode == 7'b1101111;
    wire is_jalr   = opcode == 7'b1100111;
    wire is_misc_mem = opcode == 7'b0001111;
    wire is_system = opcode == 7'b1110011;

    wire is_addi = is_op_imm & (funct3 == 3'b000);
    wire is_slti = is_op_imm & (funct3 == 3'b010);
    wire is_sltiu = is_op_imm & (funct3 == 3'b011);
    wire is_slli = is_op_imm & (funct3 == 3'b001) & (funct7 == 7'b0000000);
    wire is_xori = is_op_imm & (funct3 == 3'b100);
    wire is_srli = is_op_imm & (funct3 == 3'b101) & (funct7 == 7'b0000000);
    wire is_srai = is_op_imm & (funct3 == 3'b101) & (funct7 == 7'b0100000);
    wire is_ori  = is_op_imm & (funct3 == 3'b110);
    wire is_andi = is_op_imm & (funct3 == 3'b111);

    wire is_add = is_op & (funct3 == 3'b000) & (funct7 == 7'b0000000);
    wire is_sll = is_op & (funct3 == 3'b001) & (funct7 == 7'b0000000);
    wire is_slt = is_op & (funct3 == 3'b010) & (funct7 == 7'b0000000);
    wire is_sltu = is_op & (funct3 == 3'b011) & (funct7 == 7'b0000000);
    wire is_sub = is_op & (funct3 == 3'b000) & (funct7 == 7'b0100000);
    wire is_xor = is_op & (funct3 == 3'b100) & (funct7 == 7'b0000000);
    wire is_srl = is_op & (funct3 == 3'b101) & (funct7 == 7'b0000000);
    wire is_sra = is_op & (funct3 == 3'b101) & (funct7 == 7'b0100000);
    wire is_or  = is_op & (funct3 == 3'b110) & (funct7 == 7'b0000000);
    wire is_and = is_op & (funct3 == 3'b111) & (funct7 == 7'b0000000);

    wire is_lb  = is_load & (funct3 == 3'b000);
    wire is_lh  = is_load & (funct3 == 3'b001);
    wire is_lw  = is_load & (funct3 == 3'b010);
    wire is_lbu = is_load & (funct3 == 3'b100);
    wire is_lhu = is_load & (funct3 == 3'b101);
    wire is_sb  = is_store & (funct3 == 3'b000);
    wire is_sh  = is_store & (funct3 == 3'b001);
    wire is_sw  = is_store & (funct3 == 3'b010);
    wire is_beq = is_branch & (funct3 == 3'b000);
    wire is_bne = is_branch & (funct3 == 3'b001);
    wire is_blt = is_branch & (funct3 == 3'b100);
    wire is_bge = is_branch & (funct3 == 3'b101);
    wire is_bltu = is_branch & (funct3 == 3'b110);
    wire is_bgeu = is_branch & (funct3 == 3'b111);
    wire is_fence = is_misc_mem & (funct3 == 3'b000);
    wire is_fence_i = is_misc_mem & (funct3 == 3'b001);
    wire is_ecall = is_system & (funct3 == 3'b000) & (instr[31:20] == 12'b000000000000);
    wire is_ebreak = is_system & (funct3 == 3'b000) & (instr[31:20] == 12'b000000000001);

    wire [31:0] data_addr = rs1_value + (is_store ? imm_s : imm_i);
    wire [5:0] data_word_index = data_addr[7:2];
    wire [1:0] data_byte_offset = data_addr[1:0];
    wire [31:0] load_word = dmem[data_word_index];
    wire [31:0] store_word = dmem[data_word_index];
    wire [7:0] load_byte =
        (data_byte_offset == 2'b00) ? load_word[7:0] :
        (data_byte_offset == 2'b01) ? load_word[15:8] :
        (data_byte_offset == 2'b10) ? load_word[23:16] :
                                      load_word[31:24];
    wire [15:0] load_half = data_byte_offset[1] ? load_word[31:16] : load_word[15:0];
    wire [31:0] load_lb_value = {{24{load_byte[7]}}, load_byte};
    wire [31:0] load_lh_value = {{16{load_half[15]}}, load_half};
    wire [31:0] load_lbu_value = {24'b0, load_byte};
    wire [31:0] load_lhu_value = {16'b0, load_half};
    wire [31:0] load_value =
        is_lb ? load_lb_value :
        is_lh ? load_lh_value :
        is_lw ? load_word :
        is_lbu ? load_lbu_value :
        is_lhu ? load_lhu_value :
        32'b0;
    wire [31:0] store_byte_word =
        (data_byte_offset == 2'b00) ? {store_word[31:8], rs2_value[7:0]} :
        (data_byte_offset == 2'b01) ? {store_word[31:16], rs2_value[7:0], store_word[7:0]} :
        (data_byte_offset == 2'b10) ? {store_word[31:24], rs2_value[7:0], store_word[15:0]} :
                                      {rs2_value[7:0], store_word[23:0]};
    wire [31:0] store_half_word =
        data_byte_offset[1] ? {rs2_value[15:0], store_word[15:0]} :
                              {store_word[31:16], rs2_value[15:0]};
    wire [31:0] srai_fill = rs1_value[31] ? ~(32'hffffffff >> shamt_i) : 32'b0;
    wire [31:0] sra_fill = rs1_value[31] ? ~(32'hffffffff >> shamt_r) : 32'b0;
    wire [31:0] jalr_target = (rs1_value + imm_i) & 32'hfffffffe;
    wire [31:0] rs_sub = rs1_value - rs2_value;
    wire [31:0] imm_sub = rs1_value - imm_i;
    wire is_load_misaligned =
        (is_lw & (data_byte_offset != 2'b00)) |
        ((is_lh | is_lhu) & data_byte_offset[0]);
    wire is_store_misaligned =
        (is_sw & (data_byte_offset != 2'b00)) |
        (is_sh & data_byte_offset[0]);

    wire rs_equal = rs1_value == rs2_value;
    wire rs_signed_lt = (rs1_value[31] == rs2_value[31]) ? rs_sub[31] : rs1_value[31];
    wire rs_unsigned_lt = rs1_value < rs2_value;
    wire imm_signed_lt = (rs1_value[31] == imm_i[31]) ? imm_sub[31] : rs1_value[31];
    wire imm_unsigned_lt = rs1_value < imm_i;
    wire branch_taken =
        (is_beq & rs_equal) |
        (is_bne & (rs_equal == 1'b0)) |
        (is_blt & rs_signed_lt) |
        (is_bge & (rs_signed_lt == 1'b0)) |
        (is_bltu & rs_unsigned_lt) |
        (is_bgeu & (rs_unsigned_lt == 1'b0));
    wire is_halt = instr == 32'h0000006f;
    wire is_supported =
        is_addi | is_slti | is_sltiu | is_slli | is_xori | is_srli | is_srai | is_ori | is_andi |
        is_add | is_sll | is_slt | is_sltu | is_sub | is_xor | is_srl | is_sra | is_or | is_and |
        is_lb | is_lh | is_lw | is_lbu | is_lhu | is_sb | is_sh | is_sw |
        is_beq | is_bne | is_blt | is_bge | is_bltu | is_bgeu |
        is_lui | is_auipc | is_jal | is_jalr |
        is_fence | is_fence_i |
        is_ecall | is_ebreak;
    wire is_illegal = is_supported == 1'b0;
    wire trap_en = is_ecall | is_ebreak | is_load_misaligned | is_store_misaligned | is_illegal;
    wire [31:0] trap_cause_next =
        is_ecall ? 32'd11 :
        is_ebreak ? 32'd3 :
        is_load_misaligned ? 32'd4 :
        is_store_misaligned ? 32'd6 :
        32'd2;

    always_comb begin
        reg_write_en = 1'b0;
        store_en = 1'b0;
        next_pc = pc + 32'd4;
        rd_write_value = 32'b0;
        store_write_value = 32'b0;

        if (is_halt) begin
            next_pc = pc;
        end else if (trap_en) begin
            next_pc = pc;
        end else if (branch_taken) begin
            next_pc = pc + imm_b;
        end else if (is_jal) begin
            reg_write_en = 1'b1;
            rd_write_value = pc + 32'd4;
            next_pc = pc + imm_j;
        end else if (is_jalr) begin
            reg_write_en = 1'b1;
            rd_write_value = pc + 32'd4;
            next_pc = jalr_target;
        end else if (is_lui) begin
            reg_write_en = 1'b1;
            rd_write_value = imm_u;
        end else if (is_auipc) begin
            reg_write_en = 1'b1;
            rd_write_value = pc + imm_u;
        end else if (is_addi) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value + imm_i;
        end else if (is_slti) begin
            reg_write_en = 1'b1;
            rd_write_value = {31'b0, imm_signed_lt};
        end else if (is_sltiu) begin
            reg_write_en = 1'b1;
            rd_write_value = {31'b0, imm_unsigned_lt};
        end else if (is_slli) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value << shamt_i;
        end else if (is_xori) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value ^ imm_i;
        end else if (is_srli) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value >> shamt_i;
        end else if (is_srai) begin
            reg_write_en = 1'b1;
            rd_write_value = (rs1_value >> shamt_i) | srai_fill;
        end else if (is_ori) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value | imm_i;
        end else if (is_andi) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value & imm_i;
        end else if (is_add) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value + rs2_value;
        end else if (is_sll) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value << shamt_r;
        end else if (is_slt) begin
            reg_write_en = 1'b1;
            rd_write_value = {31'b0, rs_signed_lt};
        end else if (is_sltu) begin
            reg_write_en = 1'b1;
            rd_write_value = {31'b0, rs_unsigned_lt};
        end else if (is_sub) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value - rs2_value;
        end else if (is_xor) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value ^ rs2_value;
        end else if (is_srl) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value >> shamt_r;
        end else if (is_sra) begin
            reg_write_en = 1'b1;
            rd_write_value = (rs1_value >> shamt_r) | sra_fill;
        end else if (is_or) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value | rs2_value;
        end else if (is_and) begin
            reg_write_en = 1'b1;
            rd_write_value = rs1_value & rs2_value;
        end else if (is_lb | is_lh | is_lw | is_lbu | is_lhu) begin
            reg_write_en = 1'b1;
            rd_write_value = load_value;
        end else if (is_sb) begin
            store_en = 1'b1;
            store_write_value = store_byte_word;
        end else if (is_sh) begin
            store_en = 1'b1;
            store_write_value = store_half_word;
        end else if (is_sw) begin
            store_en = 1'b1;
            store_write_value = rs2_value;
        end else if (is_fence | is_fence_i) begin
            next_pc = pc + 32'd4;
        end
    end

    always_ff @(posedge clk) begin
        if (reset) begin
            pc <= 32'b0;
            instr_debug <= 32'b0;
            halted <= 1'b0;
            trap <= 1'b0;
            trap_cause <= 32'b0;
            regs[0] <= 32'b0;
        end else if (run && (halted == 1'b0)) begin
            instr_debug <= instr;

            if (is_halt) begin
                halted <= 1'b1;
                trap <= 1'b0;
                trap_cause <= 32'b0;
            end else if (trap_en) begin
                halted <= 1'b1;
                trap <= 1'b1;
                trap_cause <= trap_cause_next;
            end else begin
                pc <= next_pc;
                trap <= 1'b0;
                trap_cause <= 32'b0;

                if (reg_write_en && (rd_idx != 5'b00000))
                    regs[rd_idx] <= rd_write_value;

                if (store_en)
                    dmem[data_word_index] <= store_write_value;
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
