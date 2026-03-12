// ===========================================================
// 32-Bit Ripple-Carry Adder using Two 16-Bit Adders
// ===========================================================
// Inputs: inA[31:0], inB[31:0], inCarry (initial carry-in)
// Outputs: outSum[31:0], outCarry (final carry-out)
//
// This 32-bit adder is constructed by chaining two 16-bit adders:
// - Lower 16 bits: inA[15:0] + inB[15:0] + inCarry
// - Upper 16 bits: inA[31:16] + inB[31:16] + carry_from_lower
//
// Truth table (partial examples for verification):
// inA         | inB         | inCarry | outSum      | outCarry
// 0x00000000  | 0x00000000  |    0    | 0x00000000  |    0
// 0x0000FFFF  | 0x00000001  |    0    | 0x00010000  |    0
// 0xFFFFFFFF  | 0x00000001  |    0    | 0x00000000  |    1
// 0x80000000  | 0x80000000  |    0    | 0x00000000  |    1

module adder_32bit (
    input  logic [31:0] inA,      // 32-bit input A
    input  logic [31:0] inB,      // 32-bit input B
    input  logic        inCarry,  // Initial carry-in
    output logic [31:0] outSum,   // 32-bit sum output
    output logic        outCarry  // Final carry-out
);
    // Internal carry signal between the two 16-bit adders
    logic carry_lower;

    // Lower 16-bit adder (bits 15:0)
    adder_16bit u_lower16 (
        .inA(inA[15:0]),
        .inB(inB[15:0]),
        .inCarry(inCarry),
        .outSum(outSum[15:0]),
        .outCarry(carry_lower)
    );

    // Upper 16-bit adder (bits 31:16)
    adder_16bit u_upper16 (
        .inA(inA[31:16]),
        .inB(inB[31:16]),
        .inCarry(carry_lower),
        .outSum(outSum[31:16]),
        .outCarry(outCarry)
    );

endmodule