module overture_pc_8bit (
    input        clk,
    input        reset,
    input        run,
    input        jump_en,
    input  [7:0] jump_addr,
    output [7:0] pc
);

    always_ff @(posedge clk) begin
        if (reset) begin
            pc <= 8'b0;
        end else if (run) begin
            if (jump_en) begin
                pc <= jump_addr;
            end else begin
                pc <= pc + 1'b1;
            end
        end
    end

endmodule
