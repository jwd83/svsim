module top(
    output logic [7:0] all_x,
    output logic [7:0] all_z,
    output logic [3:0] explicit_x,
    output logic [3:0] explicit_z,
    output logic [3:0] mixed_bin,
    output logic [7:0] hex_all_x,
    output logic [7:0] hex_mixed,
    output logic [7:0] concat_xz,
    output logic [15:0] hex_extended
);
    assign all_x = 8'bx;
    assign all_z = 8'bz;
    assign explicit_x = 4'bxxxx;
    assign explicit_z = 4'bzzzz;
    assign mixed_bin = 4'b1x0z;
    assign hex_all_x = 8'hx;
    assign hex_mixed = 8'ha5;
    assign concat_xz = {4'bx, 4'bz};
    assign hex_extended = 16'hx1;
endmodule
