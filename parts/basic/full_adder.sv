// ===========================================================
// Full Adder using Two Half Adders
// ===========================================================
// Inputs: inA, inB, inCarry
// Outputs: outSum, outCarry
// Truth table:
// inA | inB | inCarry | outSum | outCarry
//  0  |  0  |    0    |    0   |    0
//  0  |  0  |    1    |    1   |    0
//  0  |  1  |    0    |    1   |    0
//  0  |  1  |    1    |    0   |    1
//  1  |  0  |    0    |    1   |    0
//  1  |  0  |    1    |    0   |    1
//  1  |  1  |    0    |    0   |    1
//  1  |  1  |    1    |    1   |    1
module full_adder (
    input  logic inA,
    input  logic inB,
    input  logic inCarry,
    output logic outSum,
    output logic outCarry
);
    logic sum_half1, carry_half1, carry_half2;

    // First half adder for inA and inB
    half_adder u_half1 (
        .inA(inA),
        .inB(inB),
        .outSum(sum_half1),
        .outCarry(carry_half1)
    );

    // Second half adder for sum of first half adder and inCarry
    half_adder u_half2 (
        .inA(sum_half1),
        .inB(inCarry),
        .outSum(outSum),
        .outCarry(carry_half2)
    );

    // Final carry is OR of the two half adder carries
    or_gate u_or_carry (
        .inA(carry_half1),
        .inB(carry_half2),
        .outY(outCarry)
    );
endmodule