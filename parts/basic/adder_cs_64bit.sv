// ===========================================================
// 64-Bit Carry Select Adder
// ===========================================================
// Inputs:  inA[63:0], inB[63:0], inCarry
// Outputs: outSum[63:0], outCarry
//
// Description:
// This carry select adder improves performance by computing both
// possible outcomes for the upper 32 bits in parallel:
//
// 1. Lower 32 bits are computed normally: inA[31:0] + inB[31:0] + inCarry
// 2. Upper 32 bits are computed with carry=0: inA[63:32] + inB[63:32] + 0
// 3. Upper 32 bits are computed with carry=1: inA[63:32] + inB[63:32] + 1
// 4. The actual carry-out from the lower 32 bits selects the correct upper result
//
// Performance:
// - Critical path = lower 32-bit carry select adder + multiplexer delay
// - Faster than ripple-carry, but uses more hardware than basic adders
//
// Resource Estimate (NAND equivalent count):
// - Lower 32-bit carry select adder:   ~2332 NANDs
// - Upper 32-bit carry select adder (carry=0): ~2332 NANDs  
// - Upper 32-bit carry select adder (carry=1): ~2332 NANDs
// - 32-bit result multiplexer:         ~256 NANDs
// - 1-bit carry multiplexer:           ~8 NANDs
// Total: ~7260 NANDs vs ~960 for ripple carry (~656% overhead)
// ===========================================================

module adder_cs_64bit (
    input  logic [63:0] inA,       // 64-bit input A
    input  logic [63:0] inB,       // 64-bit input B
    input  logic        inCarry,   // Initial carry-in
    output logic [63:0] outSum,    // 64-bit sum output
    output logic        outCarry   // Final carry-out
);

    // -------------------------------------------------------
    // Internal Signals
    // -------------------------------------------------------
    logic [31:0] sum_lower;
    logic        carry_from_lower;

    logic [31:0] sum_upper_c0;
    logic        carry_upper_c0;

    logic [31:0] sum_upper_c1;
    logic        carry_upper_c1;

    // -------------------------------------------------------
    // Lower 32-bit carry select adder (always computed)
    // -------------------------------------------------------
    adder_cs_32bit u_lower (
        .inA(inA[31:0]),
        .inB(inB[31:0]),
        .inCarry(inCarry),
        .outSum(sum_lower),
        .outCarry(carry_from_lower)
    );

    // -------------------------------------------------------
    // Upper 32-bit carry select adder assuming carry=0
    // -------------------------------------------------------
    adder_cs_32bit u_upper_c0 (
        .inA(inA[63:32]),
        .inB(inB[63:32]),
        .inCarry(1'b0),
        .outSum(sum_upper_c0),
        .outCarry(carry_upper_c0)
    );

    // -------------------------------------------------------
    // Upper 32-bit carry select adder assuming carry=1
    // -------------------------------------------------------
    adder_cs_32bit u_upper_c1 (
        .inA(inA[63:32]),
        .inB(inB[63:32]),
        .inCarry(1'b1),
        .outSum(sum_upper_c1),
        .outCarry(carry_upper_c1)
    );

    // -------------------------------------------------------
    // Multiplexers to select correct upper result
    // -------------------------------------------------------
    mux_2to1_32bit u_sum_mux (
        .in0(sum_upper_c0),         // Selected when carry_from_lower = 0
        .in1(sum_upper_c1),         // Selected when carry_from_lower = 1
        .sel(carry_from_lower),
        .out(outSum[63:32])
    );

    mux_2to1_1bit u_carry_mux (
        .in0(carry_upper_c0),
        .in1(carry_upper_c1),
        .sel(carry_from_lower),
        .out(outCarry)
    );

    // -------------------------------------------------------
    // Lower sum bits connect directly
    // -------------------------------------------------------
    assign outSum[31:0] = sum_lower;

endmodule