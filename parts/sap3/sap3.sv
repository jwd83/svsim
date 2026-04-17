// SAP-3 extension of the SAP-2 shared-bus CPU.
//
// Adds three logical ALU operations (AND / OR / XOR) and replaces the
// dedicated output register with a memory-mapped output port inside the
// memory module. The microcode word widens from 16 to 20 bits to carry
// four new control signals:
//
//   - alu_op_and / alu_op_or / alu_op_xor: select a logical ALU operation
//     instead of the default add/subtract path.
//   - en_select_output_port: force the memory address register to 0x10 so
//     the next bus write lands on the memory-mapped output port instead of
//     the 16-byte RAM window.
//
// The harness-visible top stays input/output only and the same shape as
// parts/sap1 and parts/sap2.

module register(
    input wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write,
    output reg [7:0] value
);
    always @(posedge clk)
        if (reset) value <= 0;
        else if (en_write) value <= bus;
endmodule

module memory(
    input wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write_mem,
    input wire en_write_mem_adr,
    input wire en_select_output_port,
    output reg [7:0] last_read,
    output reg [7:0] out_port
);
    reg [4:0] address_register;
    reg [7:0] data[0:15];

    always @(posedge clk) begin
        if (reset) begin
            address_register <= 0;
            out_port <= 0;
        end else if (en_select_output_port) begin
            address_register <= 5'h10;
        end else if (en_write_mem_adr) begin
            address_register <= {1'b0, bus[3:0]};
        end else if (en_write_mem) begin
            if (address_register[4]) out_port <= bus;
            else data[address_register[3:0]] <= bus;
        end
    end

    always @(*)
        if (address_register[4]) last_read = out_port;
        else last_read = data[address_register[3:0]];
endmodule

module rom(
    input wire [8:0] address,
    output reg [19:0] out
);
    reg [19:0] data[0:511];

    always @(*)
        out = data[address];
endmodule

module micro_instr_counter(
    input wire clk,
    input wire reset,
    output reg [2:0] count
);
    always @(posedge clk)
        if (reset) count <= 0;
        else if (count == 5) count <= 0;
        else count <= count + 1;
endmodule

module alu(
    input wire [7:0] a,
    input wire [7:0] b,
    input wire en_subtraction,
    input wire alu_op_and,
    input wire alu_op_or,
    input wire alu_op_xor,
    output reg [7:0] result,
    output reg carry_out
);
    wire [8:0] internal_sum;
    assign internal_sum =
        {1'b0, a}
        + {1'b0, en_subtraction ? ~b : b}
        + {8'b0, en_subtraction};

    always @(*) begin
        if (alu_op_and) begin
            result = a & b;
            carry_out = 1'b0;
        end else if (alu_op_or) begin
            result = a | b;
            carry_out = 1'b0;
        end else if (alu_op_xor) begin
            result = a ^ b;
            carry_out = 1'b0;
        end else begin
            result = internal_sum[7:0];
            carry_out = internal_sum[8];
        end
    end
endmodule

module bus_driver(
    input wire en_read,
    input wire [7:0] value,
    inout wire [7:0] bus
);
    assign bus = en_read ? value : 8'bz;
endmodule

module register_tile(
    inout wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write,
    input wire en_read,
    output reg [7:0] value
);
    always @(posedge clk)
        if (reset) value <= 0;
        else if (en_write) value <= bus;

    assign bus = en_read ? value : 8'bz;
endmodule

module register_pc_tile(
    inout wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write,
    input wire en_read,
    input wire en_increment_pc,
    output reg [7:0] value
);
    always @(posedge clk)
        if (reset) value <= 0;
        else if (en_increment_pc) value <= value + 1;
        else if (en_write) value <= bus;

    assign bus = en_read ? value : 8'bz;
endmodule

module register_instr_tile(
    inout wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write,
    input wire en_read,
    output reg [7:0] value
);
    always @(posedge clk)
        if (reset) value <= 0;
        else if (en_write) value <= bus;

    assign bus = en_read ? { 4'b0, value[3:0] } : 8'bz;
endmodule

module machine(
    input wire clk,
    input wire reset,
    input wire en_read_external,
    input wire [7:0] external_value,
    output wire [7:0] out_reg_out,
    output reg halted
);
    wire [7:0] bus;
    wire [7:0] alu_out;
    wire [19:0] micro;
    wire [2:0] micro_counter;
    wire en_select_output_port;
    wire alu_op_xor;
    wire alu_op_or;
    wire alu_op_and;
    wire en_subtraction;
    wire micro_done;
    wire halted_micro;
    wire en_increment_pc;
    wire en_write_a;
    wire en_read_a;
    wire en_write_b;
    wire en_write_pc;
    wire en_read_pc;
    wire en_write_instr;
    wire en_read_instr;
    wire en_write_mem;
    wire en_read_mem;
    wire en_write_mem_adr;
    wire en_read_alu;

    wire carry_out;
    reg last_zero;
    reg last_carry;

    wire [7:0] out_mem;
    wire [7:0] out_reg_a;
    wire [7:0] out_reg_b;
    wire [7:0] out_reg_pc;
    wire [7:0] out_reg_instr;

    micro_instr_counter mc(
        .clk(clk),
        .reset(reset | micro_done),
        .count(micro_counter)
    );

    register_tile a(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_a), .en_read(en_read_a),
        .value(out_reg_a)
    );
    register b(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_b), .value(out_reg_b)
    );
    register_instr_tile instr(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_instr), .en_read(en_read_instr),
        .value(out_reg_instr)
    );
    register_pc_tile pc(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_pc), .en_read(en_read_pc),
        .en_increment_pc(en_increment_pc),
        .value(out_reg_pc)
    );

    memory m(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write_mem(en_write_mem),
        .en_write_mem_adr(en_write_mem_adr),
        .en_select_output_port(en_select_output_port),
        .last_read(out_mem),
        .out_port(out_reg_out)
    );
    rom instr_decode(
        .address({ last_carry, last_zero, out_reg_instr[7:4], micro_counter }),
        .out(micro)
    );

    alu alu_inst(
        .a(out_reg_a),
        .b(out_reg_b),
        .en_subtraction(en_subtraction),
        .alu_op_and(alu_op_and),
        .alu_op_or(alu_op_or),
        .alu_op_xor(alu_op_xor),
        .result(alu_out),
        .carry_out(carry_out)
    );

    bus_driver external_bus(
        .en_read(en_read_external),
        .value(external_value),
        .bus(bus)
    );
    bus_driver alu_bus(
        .en_read(en_read_alu),
        .value(alu_out),
        .bus(bus)
    );
    bus_driver mem_bus(
        .en_read(en_read_mem),
        .value(out_mem),
        .bus(bus)
    );

    assign
        {
            en_select_output_port,
            alu_op_xor,
            alu_op_or,
            alu_op_and,
            en_subtraction,
            micro_done,
            halted_micro,
            en_increment_pc,
            en_write_a,
            en_read_a,
            en_write_b,
            en_write_pc,
            en_read_pc,
            en_write_instr,
            en_read_instr,
            en_write_mem,
            en_read_mem,
            en_write_mem_adr,
            en_read_alu
        } = micro[19:1];

    always @(posedge clk) begin
        if (reset) begin
            last_zero <= 0;
            last_carry <= 0;
            halted <= 0;
        end else begin
            if (en_read_alu) begin
                last_zero <= alu_out == 0;
                last_carry <= carry_out;
            end
            if (halted_micro) halted <= 1;
        end
    end
endmodule
