// Focused smoke test for the SAP-2 register_tile in isolation.
// Wraps one tile plus a single external bus driver so the JSON harness can
// observe en_write capture, en_read drive, floating bus, and contention.

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

module sap2_register_tile(
    input wire clk,
    input wire reset,
    input wire en_write_tile,
    input wire en_read_tile,
    input wire en_read_ext,
    input wire [7:0] ext_value,
    output wire [7:0] bus_out,
    output wire [7:0] value_out
);
    wire [7:0] bus;

    bus_driver ext(
        .en_read(en_read_ext),
        .value(ext_value),
        .bus(bus)
    );
    register_tile tile(
        .bus(bus),
        .clk(clk),
        .reset(reset),
        .en_write(en_write_tile),
        .en_read(en_read_tile),
        .value(value_out)
    );

    assign bus_out = bus;
endmodule
