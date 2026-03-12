module register_16bit (
    input clk,
    input enable,
    input [15:0] data,
    output [15:0] q
);

    // 16-bit register with enable using single always_ff block
    always_ff @(posedge clk) begin
        if (enable) begin
            q <= data;
        end
    end

endmodule