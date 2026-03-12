module register_8bit (
    input clk,
    input enable,
    input [7:0] data,
    output [7:0] q
);

    // 8-bit register with enable using single always_ff block
    always_ff @(posedge clk) begin
        if (enable) begin
            q <= data;
        end
    end

endmodule
