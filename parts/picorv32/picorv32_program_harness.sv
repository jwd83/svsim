`include "picorv32.v"

module picorv32_program_harness (
    input clk,
    input resetn,
    output trap,
    output mem_valid,
    output mem_instr,
    output [31:0] mem_addr,
    output [31:0] ram_word0,
    output [31:0] ram_word1,
    output [31:0] ram_word2,
    output [31:0] ram_word3,
    output [31:0] store_count,
    output [31:0] last_store_addr,
    output [31:0] last_store_data
);

    reg [31:0] rom [0:63];
    reg [31:0] ram_word0_reg;
    reg [31:0] ram_word1_reg;
    reg [31:0] ram_word2_reg;
    reg [31:0] ram_word3_reg;

    wire mem_ready;
    reg [31:0] mem_rdata;
    wire [31:0] mem_wdata;
    wire [3:0] mem_wstrb;

    wire rom_hit;
    wire ram_window_hit;
    wire pending_store_window_hit;

    reg [31:0] store_count_reg;
    reg [31:0] last_store_addr_reg;
    reg [31:0] last_store_data_reg;
    reg pending_store_valid_reg;
    reg [31:0] pending_store_addr_reg;
    reg [31:0] pending_store_data_reg;
    reg [3:0] pending_store_wstrb_reg;

    assign mem_ready = 1'b1;
    assign rom_hit = mem_addr[31:8] == 24'h0;
    assign ram_window_hit = mem_addr[31:8] == 24'h1 && mem_addr[7:4] == 4'h0;
    assign pending_store_window_hit =
        pending_store_addr_reg[31:8] == 24'h1 && pending_store_addr_reg[7:4] == 4'h0;

    always @(*) begin
        if (rom_hit) begin
            mem_rdata = rom[mem_addr[7:2]];
        end else if (ram_window_hit && mem_addr[3:2] == 2'd0) begin
            mem_rdata = ram_word0_reg;
        end else if (ram_window_hit && mem_addr[3:2] == 2'd1) begin
            mem_rdata = ram_word1_reg;
        end else if (ram_window_hit && mem_addr[3:2] == 2'd2) begin
            mem_rdata = ram_word2_reg;
        end else if (ram_window_hit && mem_addr[3:2] == 2'd3) begin
            mem_rdata = ram_word3_reg;
        end else begin
            mem_rdata = 32'b0;
        end
    end

    assign ram_word0 = ram_word0_reg;
    assign ram_word1 = ram_word1_reg;
    assign ram_word2 = ram_word2_reg;
    assign ram_word3 = ram_word3_reg;
    assign store_count = store_count_reg;
    assign last_store_addr = last_store_addr_reg;
    assign last_store_data = last_store_data_reg;

    always_ff @(posedge clk) begin
        if (!resetn) begin
            ram_word0_reg <= 0;
            ram_word1_reg <= 0;
            ram_word2_reg <= 0;
            ram_word3_reg <= 0;
            store_count_reg <= 0;
            last_store_addr_reg <= 0;
            last_store_data_reg <= 0;
            pending_store_valid_reg <= 1'b0;
            pending_store_addr_reg <= 0;
            pending_store_data_reg <= 0;
            pending_store_wstrb_reg <= 4'b0;
        end else begin
            if (pending_store_valid_reg && !trap && pending_store_window_hit && pending_store_addr_reg[3:2] == 2'd0) begin
                if (pending_store_wstrb_reg[0]) ram_word0_reg[7:0] <= pending_store_data_reg[7:0];
                if (pending_store_wstrb_reg[1]) ram_word0_reg[15:8] <= pending_store_data_reg[15:8];
                if (pending_store_wstrb_reg[2]) ram_word0_reg[23:16] <= pending_store_data_reg[23:16];
                if (pending_store_wstrb_reg[3]) ram_word0_reg[31:24] <= pending_store_data_reg[31:24];
                store_count_reg <= store_count_reg + 1'b1;
                last_store_addr_reg <= pending_store_addr_reg;
                last_store_data_reg <= pending_store_data_reg;
            end else if (pending_store_valid_reg && !trap && pending_store_window_hit && pending_store_addr_reg[3:2] == 2'd1) begin
                if (pending_store_wstrb_reg[0]) ram_word1_reg[7:0] <= pending_store_data_reg[7:0];
                if (pending_store_wstrb_reg[1]) ram_word1_reg[15:8] <= pending_store_data_reg[15:8];
                if (pending_store_wstrb_reg[2]) ram_word1_reg[23:16] <= pending_store_data_reg[23:16];
                if (pending_store_wstrb_reg[3]) ram_word1_reg[31:24] <= pending_store_data_reg[31:24];
                store_count_reg <= store_count_reg + 1'b1;
                last_store_addr_reg <= pending_store_addr_reg;
                last_store_data_reg <= pending_store_data_reg;
            end else if (pending_store_valid_reg && !trap && pending_store_window_hit && pending_store_addr_reg[3:2] == 2'd2) begin
                if (pending_store_wstrb_reg[0]) ram_word2_reg[7:0] <= pending_store_data_reg[7:0];
                if (pending_store_wstrb_reg[1]) ram_word2_reg[15:8] <= pending_store_data_reg[15:8];
                if (pending_store_wstrb_reg[2]) ram_word2_reg[23:16] <= pending_store_data_reg[23:16];
                if (pending_store_wstrb_reg[3]) ram_word2_reg[31:24] <= pending_store_data_reg[31:24];
                store_count_reg <= store_count_reg + 1'b1;
                last_store_addr_reg <= pending_store_addr_reg;
                last_store_data_reg <= pending_store_data_reg;
            end else if (pending_store_valid_reg && !trap && pending_store_window_hit && pending_store_addr_reg[3:2] == 2'd3) begin
                if (pending_store_wstrb_reg[0]) ram_word3_reg[7:0] <= pending_store_data_reg[7:0];
                if (pending_store_wstrb_reg[1]) ram_word3_reg[15:8] <= pending_store_data_reg[15:8];
                if (pending_store_wstrb_reg[2]) ram_word3_reg[23:16] <= pending_store_data_reg[23:16];
                if (pending_store_wstrb_reg[3]) ram_word3_reg[31:24] <= pending_store_data_reg[31:24];
                store_count_reg <= store_count_reg + 1'b1;
                last_store_addr_reg <= pending_store_addr_reg;
                last_store_data_reg <= pending_store_data_reg;
            end

            pending_store_valid_reg <= mem_valid && |mem_wstrb;
            pending_store_addr_reg <= mem_addr;
            pending_store_data_reg <= mem_wdata;
            pending_store_wstrb_reg <= mem_wstrb;
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
