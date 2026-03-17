module constant_memory_index_oob(output logic [7:0] y);
    logic [7:0] rom [0:1];
    assign y = rom[2];
endmodule
