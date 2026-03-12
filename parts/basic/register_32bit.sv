module register_32bit (
    input clk,
    input enable,
    input [31:0] data,
    output [31:0] q
);

    // 32-bit register with enable using single always_ff block
    always_ff @(posedge clk) begin
        if (enable) begin
            q <= data;
        end
    end

endmodule