// Focused shared-bus semantics smoke test for the internal SAP-2 bus model.

module bus_driver(
    input wire en_read,
    input wire value,
    inout wire bus
);
    wire float_bus;

    assign bus = en_read ? value : float_bus;
endmodule

module sap2_bus_semantics(
    input wire drive_low,
    input wire drive_high,
    output wire bus_out
);
    wire bus;

    bus_driver low(
        .en_read(drive_low),
        .value(1'b0),
        .bus(bus)
    );
    bus_driver high(
        .en_read(drive_high),
        .value(1'b1),
        .bus(bus)
    );

    assign bus_out = bus;
endmodule
