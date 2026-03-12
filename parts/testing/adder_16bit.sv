// ===========================================================
// 16-Bit Ripple-Carry Adder using Two 8-Bit Adders
// ===========================================================
// Inputs: inA[15:0], inB[15:0], inCarry (initial carry-in)
// Outputs: outSum[15:0], outCarry (final carry-out)
//
// This 16-bit adder is constructed by chaining two 8-bit adders:
// - Lower 8 bits: inA[7:0] + inB[7:0] + inCarry
// - Upper 8 bits: inA[15:8] + inB[15:8] + carry_from_lower
//
// Truth table (partial examples for verification):
// inA     | inB     | inCarry | outSum  | outCarry
// 0x0000  | 0x0000  |    0    | 0x0000  |    0
// 0x00FF  | 0x0001  |    0    | 0x0100  |    0
// 0xFFFF  | 0x0001  |    0    | 0x0000  |    1
// 0x8000  | 0x8000  |    0    | 0x0000  |    1

module adder_16bit (
    input  logic [15:0] inA,      // 16-bit input A
    input  logic [15:0] inB,      // 16-bit input B
    input  logic        inCarry,  // Initial carry-in
    output logic [15:0] outSum,   // 16-bit sum output
    output logic        outCarry  // Final carry-out
);
    // Internal carry signal between the two 8-bit adders
    logic carry_lower;

    // Lower 8-bit adder (bits 7:0)
    adder_8bit u_lower8 (
        .inA(inA[7:0]),
        .inB(inB[7:0]),
        .inCarry(inCarry),
        .outSum(outSum[7:0]),
        .outCarry(carry_lower)
    );

    // Upper 8-bit adder (bits 15:8)
    adder_8bit u_upper8 (
        .inA(inA[15:8]),
        .inB(inB[15:8]),
        .inCarry(carry_lower),
        .outSum(outSum[15:8]),
        .outCarry(outCarry)
    );

endmodule