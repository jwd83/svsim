// SAP-2 scaffold for the future resolved-bus port.
// This file intentionally preserves the harness-facing top contract only.

module machine(
    input wire clk,
    input wire reset,
    input wire en_read_external,
    input wire [7:0] external_value,
    output wire [7:0] out_reg_out,
    output reg halted
);
    // Placeholder behavior until the structural SAP-2 core lands.
    assign out_reg_out = 8'h00;

    always @(*) begin
        halted = 1'b0;
    end
endmodule
