// ===========================================================
// 4-Bit Ripple-Carry Adder using Full Adders
// ===========================================================
// Inputs: inA[3:0], inB[3:0], inCarry (initial carry-in)
// Outputs: outSum[3:0], outCarry (final carry-out)
// Truth table (partial example for brevity):
// inA  | inB  | inCarry | outSum | outCarry
// 0000 | 0000 |    0    | 0000   |    0
// 0001 | 0010 |    0    | 0011   |    0
// 1111 | 1111 |    0    | 1110   |    1
module adder_4bit (
    input  logic [3:0] inA,      // 4-bit input A
    input  logic [3:0] inB,      // 4-bit input B
    input  logic       inCarry,  // Initial carry-in
    output logic [3:0] outSum,   // 4-bit sum output
    output logic       outCarry  // Final carry-out
);
    // Internal carry signals between full adders
    logic carry0, carry1, carry2;

    // Full adder for bit 0 (least significant bit)
    full_adder u_fa0 (
        .inA(inA[0]),
        .inB(inB[0]),
        .inCarry(inCarry),
        .outSum(outSum[0]),
        .outCarry(carry0)
    );

    // Full adder for bit 1
    full_adder u_fa1 (
        .inA(inA[1]),
        .inB(inB[1]),
        .inCarry(carry0),
        .outSum(outSum[1]),
        .outCarry(carry1)
    );

    // Full adder for bit 2
    full_adder u_fa2 (
        .inA(inA[2]),
        .inB(inB[2]),
        .inCarry(carry1),
        .outSum(outSum[2]),
        .outCarry(carry2)
    );

    // Full adder for bit 3 (most significant bit)
    full_adder u_fa3 (
        .inA(inA[3]),
        .inB(inB[3]),
        .inCarry(carry2),
        .outSum(outSum[3]),
        .outCarry(outCarry)
    );
endmodule
