`include "picorv32.v"

module picorv32_smoke (
    input clk,
    input resetn,
    output trap,
    output mem_valid,
    output mem_instr,
    output [31:0] mem_addr,
    output store_seen,
    output [31:0] store_addr,
    output [31:0] store_data
);

    reg [31:0] rom [0:15];
    wire mem_ready;
    wire [31:0] mem_rdata;
    wire [31:0] mem_wdata;
    wire [3:0] mem_wstrb;

    reg store_seen_reg;
    reg [31:0] store_addr_reg;
    reg [31:0] store_data_reg;

    assign mem_ready = 1'b1;
    assign mem_rdata = rom[mem_addr[5:2]];
    assign store_seen = store_seen_reg;
    assign store_addr = store_addr_reg;
    assign store_data = store_data_reg;

    always_ff @(posedge clk) begin
        if (!resetn) begin
            store_seen_reg <= 0;
            store_addr_reg <= 0;
            store_data_reg <= 0;
        end else if (mem_valid && |mem_wstrb) begin
            store_seen_reg <= 1'b1;
            store_addr_reg <= mem_addr;
            store_data_reg <= mem_wdata;
        end
    end

    picorv32 uut (
        .clk(clk),
        .resetn(resetn),
        .trap(trap),
        .mem_valid(mem_valid),
        .mem_instr(mem_instr),
        .mem_ready(mem_ready),
        .mem_addr(mem_addr),
        .mem_wdata(mem_wdata),
        .mem_wstrb(mem_wstrb),
        .mem_rdata(mem_rdata),
        .pcpi_wr(1'b0),
        .pcpi_rd(32'b0),
        .pcpi_wait(1'b0),
        .pcpi_ready(1'b0),
        .irq(32'b0)
    );

endmodule
