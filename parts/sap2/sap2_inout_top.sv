// SAP-2 variant that exposes the shared bus directly as a top-level `inout`.
// The JSON harness participates in bus resolution as a real external driver:
// it can release the bus with `8'bz`, drive a value, or contend with the
// internal register tiles' drivers.

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

module sap2_inout_top(
    input wire clk,
    input wire reset,
    input wire en_write_a,
    input wire en_read_a,
    input wire en_write_b,
    input wire en_read_b,
    inout wire [7:0] bus,
    output wire [7:0] reg_a,
    output wire [7:0] reg_b
);
    register_tile a(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_a), .en_read(en_read_a),
        .value(reg_a)
    );
    register_tile b(
        .bus(bus), .clk(clk), .reset(reset),
        .en_write(en_write_b), .en_read(en_read_b),
        .value(reg_b)
    );
endmodule
