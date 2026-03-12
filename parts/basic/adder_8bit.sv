module adder_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    input  logic       inCarry,
    output logic [7:0] outSum,
    output logic       outCarry
);
    logic carry_lower;

    // Connect lower 4 bits as vectors
    adder_4bit u_lower4 (
        .inA(inA[3:0]),
        .inB(inB[3:0]),
        .inCarry(inCarry),
        .outSum(outSum[3:0]),
        .outCarry(carry_lower)
    );

    // Connect upper 4 bits as vectors
    adder_4bit u_upper4 (
        .inA(inA[7:4]),
        .inB(inB[7:4]),
        .inCarry(carry_lower),
        .outSum(outSum[7:4]),
        .outCarry(outCarry)
    );
endmodule
