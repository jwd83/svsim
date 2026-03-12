// ===========================================================
// 32-Bit Carry Select Adder
// ===========================================================
// Inputs:  inA[31:0], inB[31:0], inCarry
// Outputs: outSum[31:0], outCarry
//
// Description:
// This carry select adder improves performance by computing both
// possible outcomes for the upper 16 bits in parallel:
//
// 1. Lower 16 bits are computed normally: inA[15:0] + inB[15:0] + inCarry
// 2. Upper 16 bits are computed with carry=0: inA[31:16] + inB[31:16] + 0
// 3. Upper 16 bits are computed with carry=1: inA[31:16] + inB[31:16] + 1
// 4. The actual carry-out from the lower 16 bits selects the correct upper result
//
// Performance:
// - Critical path = lower 16-bit carry select adder + multiplexer delay
// - Faster than ripple-carry, but uses more hardware than basic adders
//
// Resource Estimate (NAND equivalent count):
// - Lower 16-bit carry select adder:   ~732 NANDs
// - Upper 16-bit carry select adder (carry=0): ~732 NANDs  
// - Upper 16-bit carry select adder (carry=1): ~732 NANDs
// - 16-bit result multiplexer:         ~128 NANDs
// - 1-bit carry multiplexer:           ~8 NANDs
// Total: ~2332 NANDs vs ~480 for ripple carry (~386% overhead)
// ===========================================================

module adder_cs_32bit (
    input  logic [31:0] inA,       // 32-bit input A
    input  logic [31:0] inB,       // 32-bit input B
    input  logic        inCarry,   // Initial carry-in
    output logic [31:0] outSum,    // 32-bit sum output
    output logic        outCarry   // Final carry-out
);

    // -------------------------------------------------------
    // Internal Signals
    // -------------------------------------------------------
    logic [15:0] sum_lower;
    logic        carry_from_lower;

    logic [15:0] sum_upper_c0;
    logic        carry_upper_c0;

    logic [15:0] sum_upper_c1;
    logic        carry_upper_c1;

    // -------------------------------------------------------
    // Lower 16-bit carry select adder (always computed)
    // -------------------------------------------------------
    adder_cs_16bit u_lower (
        .inA(inA[15:0]),
        .inB(inB[15:0]),
        .inCarry(inCarry),
        .outSum(sum_lower),
        .outCarry(carry_from_lower)
    );

    // -------------------------------------------------------
    // Upper 16-bit carry select adder assuming carry=0
    // -------------------------------------------------------
    adder_cs_16bit u_upper_c0 (
        .inA(inA[31:16]),
        .inB(inB[31:16]),
        .inCarry(1'b0),
        .outSum(sum_upper_c0),
        .outCarry(carry_upper_c0)
    );

    // -------------------------------------------------------
    // Upper 16-bit carry select adder assuming carry=1
    // -------------------------------------------------------
    adder_cs_16bit u_upper_c1 (
        .inA(inA[31:16]),
        .inB(inB[31:16]),
        .inCarry(1'b1),
        .outSum(sum_upper_c1),
        .outCarry(carry_upper_c1)
    );

    // -------------------------------------------------------
    // Multiplexers to select correct upper result
    // -------------------------------------------------------
    mux_2to1_16bit u_sum_mux (
        .in0(sum_upper_c0),         // Selected when carry_from_lower = 0
        .in1(sum_upper_c1),         // Selected when carry_from_lower = 1
        .sel(carry_from_lower),
        .out(outSum[31:16])
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
    assign outSum[15:0] = sum_lower;

endmodule