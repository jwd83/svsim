module simple8_programmable (
    input  logic clk,
    input  logic reset,
    output logic [4:0] pc_out,
    output logic       z_out,
    output logic [7:0] r0_out,
    output logic [7:0] r1_out,
    output logic [7:0] r2_out,
    output logic [7:0] r3_out,
    output logic [7:0] out_port,
    output logic [7:0] ram_10_out
);

    logic [4:0]  pc;
    logic [7:0]  regfile [0:3];
    logic        z_flag;

    logic [15:0] instr_mem [0:31];
    logic [7:0]  data_mem  [0:31];

    logic [15:0] instr;
    logic [3:0]  opcode;
    logic [1:0]  rd_idx;
    logic [1:0]  rs_idx;
    logic [7:0]  imm8;
    logic [4:0]  addr;

    logic [7:0]  rd_val;
    logic [7:0]  rs_val;
    logic [7:0]  alu_result;
    logic [7:0]  writeback;
    logic [7:0]  mem_read;

    logic [4:0]  next_pc;
    logic        next_z;
    logic        reg_we;
    logic        mem_we;

    assign instr = instr_mem[pc];

    assign opcode = instr[15:12];
    assign rd_idx = instr[11:10];
    assign rs_idx = instr[9:8];
    assign imm8   = instr[7:0];
    assign addr   = imm8[4:0];

    assign rd_val = regfile[rd_idx];
    assign rs_val = regfile[rs_idx];
    assign mem_read = data_mem[addr];

    assign pc_out     = pc;
    assign z_out      = z_flag;
    assign r0_out     = regfile[0];
    assign r1_out     = regfile[1];
    assign r2_out     = regfile[2];
    assign r3_out     = regfile[3];
    assign out_port   = regfile[3];
    assign ram_10_out = data_mem[5'h10];

    always_comb begin
        next_pc    = pc + 5'd1;
        alu_result = 8'd0;
        writeback  = 8'd0;
        reg_we     = 1'b0;
        mem_we     = 1'b0;
        next_z     = z_flag;

        case (opcode)
            4'h0: begin
            end
            4'h1: begin
                alu_result = rd_val + rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end
            4'h2: begin
                alu_result = rd_val - rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end
            4'h3: begin
                alu_result = rd_val & rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end
            4'h4: begin
                alu_result = rd_val | rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end
            4'h5: begin
                alu_result = rd_val ^ rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end
            4'h6: begin
                writeback = imm8;
                reg_we    = 1'b1;
                next_z    = (imm8 == 8'd0);
            end
            4'h7: begin
                writeback = mem_read;
                reg_we    = 1'b1;
                next_z    = (mem_read == 8'd0);
            end
            4'h8: begin
                mem_we = 1'b1;
            end
            4'h9: begin
                next_pc = addr;
            end
            4'hA: begin
                if (z_flag)
                    next_pc = addr;
            end
            default: begin
            end
        endcase
    end

    always_ff @(posedge clk or posedge reset) begin
        if (reset) begin
            pc     <= 5'd0;
            z_flag <= 1'b0;

            regfile[0] <= 8'd0;
            regfile[1] <= 8'd0;
            regfile[2] <= 8'd0;
            regfile[3] <= 8'd0;

            for (int i = 0; i < 32; i++) begin
                data_mem[i] <= 8'd0;
            end
        end else begin
            pc     <= next_pc;
            z_flag <= next_z;

            if (reg_we)
                regfile[rd_idx] <= writeback;

            if (mem_we)
                data_mem[addr] <= rd_val;
        end
    end

endmodule
