// ===========================================================
// 1-Bit Register with Clock, Enable, and Data
// ===========================================================
// Inputs: clk (clock), enable, data
// Outputs: q (stored value)
//
// Behavior:
// - On positive edge of clk: if enable is high, q <= data
// - Otherwise: q retains previous value
//
// This is a basic D flip-flop with enable functionality

module register_1bit (
    input  logic clk,      // Clock signal
    input  logic enable,   // Enable signal (when high, allows updates)
    input  logic data,     // Data input
    output logic q         // Stored output value
);
    // Sequential logic block
    always_ff @(posedge clk) begin
        if (enable) begin
            q <= data;
        end
        // If enable is low, q retains its previous value
    end

endmodule