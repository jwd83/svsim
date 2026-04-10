module top(
    input logic inA,
    input logic inB,
    output logic passA,
    output logic eqRaw,
    output logic ternaryMix,
    output tri floatZ
);
    assign passA = inA;
    assign eqRaw = (inA == 1'b1);
    assign ternaryMix = inA ? 1'b1 : inB;
endmodule
