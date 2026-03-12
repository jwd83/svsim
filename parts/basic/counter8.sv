module counter8 (
    input clk,
    input reset,
    input enable,
    output [7:0] count
);

    // 8-bit counter with arithmetic increment
    always_ff @(posedge clk) begin
        if (reset) begin
            count <= 0;
        end else if (enable) begin
            count <= count + 1;
        end
        // If enable is low, count holds its current value
    end

endmodule
