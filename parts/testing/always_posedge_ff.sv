module always_posedge_ff (
    input clk,
    input enable,
    input [7:0] data,
    output [7:0] q
);

    // Verilog-2001 style sequential block using @(posedge clk)
    always @(posedge clk) begin
        if (enable) begin
            q <= data;
        end
    end

endmodule
