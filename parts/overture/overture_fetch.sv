module overture_fetch (
    input  [7:0] addr,
    output [7:0] data
);

    reg [7:0] rom [255:0];

    assign data = rom[addr];

endmodule
