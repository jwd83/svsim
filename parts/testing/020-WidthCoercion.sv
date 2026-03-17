module pass4 (
    input [3:0] in,
    output [3:0] out
);
    assign out = in;
endmodule

module pass2 (
    input [1:0] in,
    output [1:0] out
);
    assign out = in;
endmodule

module bit_driver (
    output out
);
    assign out = 1'b1;
endmodule

module bus_driver (
    output [4:0] out
);
    assign out = 5'b10101;
endmodule

module top_module (
    input a,
    input [7:0] wide_in,
    output [3:0] assign_widened,
    output [1:0] assign_narrowed,
    output [3:0] child_input_widened,
    output [1:0] child_input_narrowed,
    output [5:0] child_output_widened,
    output [2:0] child_output_narrowed
);
    assign assign_widened = a;
    assign assign_narrowed = wide_in;

    pass4 widen_input (
        .in(a),
        .out(child_input_widened)
    );

    pass2 narrow_input (
        .in(wide_in),
        .out(child_input_narrowed)
    );

    bit_driver widen_output (
        .out(child_output_widened)
    );

    bus_driver narrow_output (
        .out(child_output_narrowed)
    );
endmodule
