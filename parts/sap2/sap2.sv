// SAP-2 shared-bus variant of the maintained SAP-1 harness contract.
// The public top-level API stays input/output only, but internal bus
// participants now use real `inout` ports and high-impedance drive.

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

module registerpc(
    input wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write,
    input wire en_increment_pc,
    output reg [7:0] value
);
    always @(posedge clk)
        if (reset) value <= 0;
        else if (en_increment_pc) value <= value + 1;
        else if (en_write) value <= bus;
endmodule

module memory(
    input wire [7:0] bus,
    input wire clk,
    input wire reset,
    input wire en_write_mem,
    input wire en_write_mem_adr,
    output reg [7:0] last_read
);
    reg [3:0] address_register;
    reg [7:0] data[0:15];

    always @(posedge clk) begin
        if (reset) address_register <= 0;
        else if (en_write_mem_adr) address_register <= bus;
        else if (en_write_mem) data[address_register] <= bus;
    end

    always @(*)
        last_read = data[address_register];
endmodule

module rom(
    input wire [8:0] address,
    output reg [15:0] out
);
    reg [15:0] data[0:511];

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

module add_carry(
    input wire [7:0] a,
    input wire [7:0] b,
    input wire carry_in,
    output wire [7:0] sum,
    output wire carry_out
);
    wire [8:0] internal_sum;

    assign internal_sum = {1'b0, a} + {1'b0, b} + {8'b0, carry_in};
    assign sum = internal_sum[7:0];
    assign carry_out = internal_sum[8];
endmodule

module bus_driver(
    input wire en_read,
    input wire [7:0] value,
    inout wire [7:0] bus
);
    wire [7:0] float_bus;

    assign bus = en_read ? value : float_bus;
endmodule

module machine(
    input wire clk,
    input wire reset,
    input wire en_read_external,
    input wire [7:0] external_value,
    output wire [7:0] out_reg_out,
    output reg halted
);
    // A shared resolved bus lets the corpus observe floating and contention
    // behavior without changing the established harness-visible top contract.
    wire [7:0] bus;
    wire [7:0] alu;
    wire [15:0] micro;
    wire [2:0] micro_counter;
    wire en_write_a;
    wire en_read_a;
    wire en_write_b;
    wire en_write_pc;
    wire en_read_pc;
    wire en_increment_pc;
    wire en_write_instr;
    wire en_read_instr;
    wire en_write_mem;
    wire en_read_mem;
    wire en_write_mem_adr;
    wire en_read_alu;
    wire micro_done;
    wire en_subtraction;
    wire en_write_out;
    wire halted_micro;

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

    register a(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_a), .value(out_reg_a)
    );
    register b(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_b), .value(out_reg_b)
    );
    register out_r(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_out), .value(out_reg_out)
    );
    register instr(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_instr), .value(out_reg_instr)
    );
    registerpc pc(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_pc), .en_increment_pc(en_increment_pc),
        .value(out_reg_pc)
    );

    memory m(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write_mem(en_write_mem), .en_write_mem_adr(en_write_mem_adr),
        .last_read(out_mem)
    );
    rom instr_decode(
        .address({ last_carry, last_zero, out_reg_instr[7:4], micro_counter }),
        .out(micro)
    );

    add_carry adc(
        .a(out_reg_a),
        .b(en_subtraction ? ~out_reg_b : out_reg_b),
        .carry_in(en_subtraction),
        .sum(alu),
        .carry_out(carry_out)
    );

    bus_driver external_bus(
        .en_read(en_read_external),
        .value(external_value),
        .bus(bus)
    );
    bus_driver alu_bus(
        .en_read(en_read_alu),
        .value(alu),
        .bus(bus)
    );
    bus_driver instr_bus(
        .en_read(en_read_instr),
        .value({ 4'b0, out_reg_instr[3:0] }),
        .bus(bus)
    );
    bus_driver mem_bus(
        .en_read(en_read_mem),
        .value(out_mem),
        .bus(bus)
    );
    bus_driver reg_a_bus(
        .en_read(en_read_a),
        .value(out_reg_a),
        .bus(bus)
    );
    bus_driver pc_bus(
        .en_read(en_read_pc),
        .value(out_reg_pc),
        .bus(bus)
    );

    assign
        {
            en_write_out,
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
        } = micro;

    always @(posedge clk) begin
        if (reset) begin
            last_zero <= 0;
            last_carry <= 0;
            halted <= 0;
        end else begin
            if (en_read_alu) begin
                last_zero <= alu == 0;
                last_carry <= carry_out;
            end
            if (halted_micro) halted <= 1;
        end
    end
endmodule
