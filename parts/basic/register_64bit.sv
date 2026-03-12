module register_64bit (
    input clk,
    input enable,
    input [63:0] data,
    output [63:0] q
);

    // 64-bit register with enable using single always_ff block
    always_ff @(posedge clk) begin
        if (enable) begin
            q <= data;
        end
    end

endmodule