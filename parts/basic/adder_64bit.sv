// ===========================================================
// 64-Bit Ripple-Carry Adder using Two 32-Bit Adders
// ===========================================================
// Inputs: inA[63:0], inB[63:0], inCarry (initial carry-in)
// Outputs: outSum[63:0], outCarry (final carry-out)
//
// This 64-bit adder is constructed by chaining two 32-bit adders:
// - Lower 32 bits: inA[31:0] + inB[31:0] + inCarry
// - Upper 32 bits: inA[63:32] + inB[63:32] + carry_from_lower
//
// Truth table (partial examples for verification):
// inA                 | inB                 | inCarry | outSum              | outCarry
// 0x0000000000000000  | 0x0000000000000000  |    0    | 0x0000000000000000  |    0
// 0x00000000FFFFFFFF  | 0x0000000000000001  |    0    | 0x0000000100000000  |    0
// 0xFFFFFFFFFFFFFFFF  | 0x0000000000000001  |    0    | 0x0000000000000000  |    1
// 0x8000000000000000  | 0x8000000000000000  |    0    | 0x0000000000000000  |    1

module adder_64bit (
    input  logic [63:0] inA,      // 64-bit input A
    input  logic [63:0] inB,      // 64-bit input B
    input  logic        inCarry,  // Initial carry-in
    output logic [63:0] outSum,   // 64-bit sum output
    output logic        outCarry  // Final carry-out
);
    // Internal carry signal between the two 32-bit adders
    logic carry_lower;

    // Lower 32-bit adder (bits 31:0)
    adder_32bit u_lower32 (
        .inA(inA[31:0]),
        .inB(inB[31:0]),
        .inCarry(inCarry),
        .outSum(outSum[31:0]),
        .outCarry(carry_lower)
    );

    // Upper 32-bit adder (bits 63:32)
    adder_32bit u_upper32 (
        .inA(inA[63:32]),
        .inB(inB[63:32]),
        .inCarry(carry_lower),
        .outSum(outSum[63:32]),
        .outCarry(outCarry)
    );

endmodule