// ============================================================================
//  Simple8 — A tiny single-cycle CPU for learning computer architecture
//
//  Quick overview
//  ──────────────
//  • 8-bit data, 16-bit instructions
//  • 4 general-purpose registers: R0, R1, R2, R3
//  • 32 words of instruction memory (ROM) — loaded at reset
//  • 32 bytes of data memory (RAM)
//  • One flag: Z (zero) — set when a result equals zero
//  • 11 instructions (see table below)
//
//  Instruction encoding
//  ──────────────────────────────────────────────────────
//   15  14  13  12 | 11  10 |  9   8 |  7  6  5  4  3  2  1  0
//      opcode      |   rd   |   rs   |         imm8
//  ──────────────────────────────────────────────────────
//
//  Instruction set
//  ──────────────────────────────────────────────────────
//   Opcode  Name   Action                  Flags
//   0x0     NOP    do nothing              —
//   0x1     ADD    rd = rd + rs            Z
//   0x2     SUB    rd = rd - rs            Z
//   0x3     AND    rd = rd & rs            Z
//   0x4     OR     rd = rd | rs            Z
//   0x5     XOR    rd = rd ^ rs            Z
//   0x6     LDI    rd = imm8               Z
//   0x7     LD     rd = RAM[imm8]          Z
//   0x8     ST     RAM[imm8] = rd          —
//   0x9     JMP    pc = imm8               —
//   0xA     JZ     if Z: pc = imm8         —
//  ──────────────────────────────────────────────────────
//
//  Note: LDI uses the full 8-bit immediate (values 0–255).
//        Memory and jump addresses use the low 5 bits (addresses 0–31).
//
// ============================================================================

module simple8_cpu (
    input  logic clk,
    input  logic reset
);

    // ─── Architectural state ─────────────────────────────────────────────

    logic [4:0]  pc;                    // program counter (addresses 0–31)
    logic [7:0]  regfile [0:3];         // registers R0–R3
    logic        z_flag;                // zero flag

    // ─── Memory ──────────────────────────────────────────────────────────

    logic [15:0] instr_mem [0:31];      // 32 × 16-bit instruction memory
    logic [7:0]  data_mem  [0:31];      // 32 × 8-bit  data memory

    // ─── Instruction fields ──────────────────────────────────────────────

    logic [15:0] instr;                 // the full 16-bit instruction word
    logic [3:0]  opcode;                // bits [15:12]
    logic [1:0]  rd_idx;                // bits [11:10] — destination register
    logic [1:0]  rs_idx;                // bits  [9:8]  — source register
    logic [7:0]  imm8;                  // bits  [7:0]  — immediate value
    logic [4:0]  addr;                  // low 5 bits of imm8 (memory/jump address)

    // ─── Internal wires ──────────────────────────────────────────────────

    logic [7:0]  rd_val;                // current value of the destination register
    logic [7:0]  rs_val;                // current value of the source register
    logic [7:0]  alu_result;            // output of the ALU
    logic [7:0]  writeback;             // value to write back to a register
    logic [7:0]  mem_read;              // value read from data memory

    logic [4:0]  next_pc;               // what pc will become next cycle
    logic        next_z;                // what z_flag will become next cycle
    logic        reg_we;                // register write enable
    logic        mem_we;                // memory write enable

    // =====================================================================
    //  FETCH — read the instruction at the current PC
    // =====================================================================

    assign instr = instr_mem[pc];

    // =====================================================================
    //  DECODE — break the instruction into its fields
    // =====================================================================

    assign opcode = instr[15:12];
    assign rd_idx = instr[11:10];
    assign rs_idx = instr[9:8];
    assign imm8   = instr[7:0];
    assign addr   = imm8[4:0];         // 5-bit address for memory and jumps

    // Read the two register operands
    assign rd_val = regfile[rd_idx];
    assign rs_val = regfile[rs_idx];

    // Read data memory (always available; only used by LD)
    assign mem_read = data_mem[addr];

    // =====================================================================
    //  EXECUTE — figure out what this instruction does
    // =====================================================================

    always_comb begin
        // Defaults: advance PC by 1, don't write anything, keep the flag.
        next_pc    = pc + 5'd1;
        alu_result = 8'd0;
        writeback  = 8'd0;
        reg_we     = 1'b0;
        mem_we     = 1'b0;
        next_z     = z_flag;

        case (opcode)

            4'h0: begin // NOP — do nothing
            end

            // ── Arithmetic & logic ────────────────────────────────────
            4'h1: begin // ADD rd, rs
                alu_result = rd_val + rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end

            4'h2: begin // SUB rd, rs
                alu_result = rd_val - rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end

            4'h3: begin // AND rd, rs
                alu_result = rd_val & rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end

            4'h4: begin // OR rd, rs
                alu_result = rd_val | rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end

            4'h5: begin // XOR rd, rs
                alu_result = rd_val ^ rs_val;
                writeback  = alu_result;
                reg_we     = 1'b1;
                next_z     = (alu_result == 8'd0);
            end

            // ── Data movement ─────────────────────────────────────────
            4'h6: begin // LDI rd, imm8 — load an immediate value
                writeback = imm8;
                reg_we    = 1'b1;
                next_z    = (imm8 == 8'd0);
            end

            4'h7: begin // LD rd, [addr] — load from data memory
                writeback = mem_read;
                reg_we    = 1'b1;
                next_z    = (mem_read == 8'd0);
            end

            4'h8: begin // ST [addr], rd — store to data memory
                mem_we = 1'b1;
            end

            // ── Control flow ──────────────────────────────────────────
            4'h9: begin // JMP addr — unconditional jump
                next_pc = addr;
            end

            4'hA: begin // JZ addr — jump if zero flag is set
                if (z_flag)
                    next_pc = addr;
            end

            default: begin // Unknown opcode — treat as NOP
            end

        endcase
    end

    // =====================================================================
    //  WRITE BACK — update state on the rising edge of the clock
    // =====================================================================

    always_ff @(posedge clk or posedge reset) begin
        if (reset) begin
            // Clear the processor state
            pc     <= 5'd0;
            z_flag <= 1'b0;

            regfile[0] <= 8'd0;
            regfile[1] <= 8'd0;
            regfile[2] <= 8'd0;
            regfile[3] <= 8'd0;

            // Clear data memory
            for (int i = 0; i < 32; i++) begin
                data_mem[i] <= 8'd0;
            end

            // ─────────────────────────────────────────────────────────
            //  Example program: adds two numbers, stores the result,
            //  loads it back, subtracts, and branches on zero.
            //
            //  Addr  Instruction       Meaning
            //  ────  ────────────────  ──────────────────────────
            //  0x00  LDI R0, 5         R0 = 5
            //  0x01  LDI R1, 3         R1 = 3
            //  0x02  ADD R0, R1        R0 = R0 + R1 = 8
            //  0x03  ST  [0x10], R0    RAM[16] = 8
            //  0x04  LD  R2, [0x10]    R2 = RAM[16] = 8
            //  0x05  SUB R2, R1        R2 = R2 - R1 = 5
            //  0x06  LDI R3, 5         R3 = 5
            //  0x07  SUB R2, R3        R2 = R2 - R3 = 0  → Z=1
            //  0x08  JZ  0x0A          Z is set, so jump to 0x0A
            //  0x09  JMP 0x09          (skipped — would loop forever)
            //  0x0A  NOP               done!
            // ─────────────────────────────────────────────────────────

            instr_mem[5'h00] <= 16'h6005;  // LDI R0, 5
            instr_mem[5'h01] <= 16'h6503;  // LDI R1, 3
            instr_mem[5'h02] <= 16'h1100;  // ADD R0, R1
            instr_mem[5'h03] <= 16'h8010;  // ST  [0x10], R0
            instr_mem[5'h04] <= 16'h7810;  // LD  R2, [0x10]
            instr_mem[5'h05] <= 16'h2900;  // SUB R2, R1
            instr_mem[5'h06] <= 16'h6F05;  // LDI R3, 5
            instr_mem[5'h07] <= 16'h2B00;  // SUB R2, R3
            instr_mem[5'h08] <= 16'hA00A;  // JZ  0x0A
            instr_mem[5'h09] <= 16'h9009;  // JMP 0x09
            instr_mem[5'h0A] <= 16'h0000;  // NOP

            for (int i = 11; i < 32; i++) begin
                instr_mem[i] <= 16'h0000;
            end

        end else begin
            // Normal operation: advance the machine by one step
            pc     <= next_pc;
            z_flag <= next_z;

            if (reg_we)
                regfile[rd_idx] <= writeback;

            if (mem_we)
                data_mem[addr] <= rd_val;
        end
    end

endmodule
