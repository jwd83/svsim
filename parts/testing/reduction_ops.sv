module reduction_ops (
    input  [3:0] a,
    output       red_or,
    output       red_and,
    output       red_xor
);

    assign red_or  = |a;
    assign red_and = &a;
    assign red_xor = ^a;

endmodule
