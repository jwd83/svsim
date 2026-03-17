module top_module (
    input [7:0] in,
    input [3:0] shamt,
    output [7:0] left_shifted,
    output [7:0] right_shifted,
    output [7:0] right_past_width
);
    assign left_shifted = in << shamt;
    assign right_shifted = in >> shamt;
    assign right_past_width = in >> 4'd8;
endmodule
