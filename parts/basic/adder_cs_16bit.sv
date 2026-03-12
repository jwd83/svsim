// ===========================================================
// 16-Bit Carry Select Adder
// ===========================================================
// Inputs:  inA[15:0], inB[15:0], inCarry
// Outputs: outSum[15:0], outCarry
//
// Description:
// This carry select adder improves performance by computing both
// possible outcomes for the upper 8 bits in parallel:
//
// 1. Lower 8 bits are computed normally: inA[7:0] + inB[7:0] + inCarry
// 2. Upper 8 bits are computed with carry=0: inA[15:8] + inB[15:8] + 0
// 3. Upper 8 bits are computed with carry=1: inA[15:8] + inB[15:8] + 1
// 4. The actual carry-out from the lower 8 bits selects the correct upper result
//
// Performance:
// - Critical path = lower 8-bit carry select adder + multiplexer delay
// - Faster than ripple-carry, but uses more hardware than basic adders
//
// Resource Estimate (NAND equivalent count):
// - Lower 8-bit carry select adder:    ~220 NANDs
// - Upper 8-bit carry select adder (carry=0): ~220 NANDs  
// - Upper 8-bit carry select adder (carry=1): ~220 NANDs
// - 8-bit result multiplexer:          ~64 NANDs
// - 1-bit carry multiplexer:           ~8 NANDs
// Total: ~732 NANDs vs ~240 for ripple carry (~205% overhead)
// ===========================================================

module adder_cs_16bit (
    input  logic [15:0] inA,       // 16-bit input A
    input  logic [15:0] inB,       // 16-bit input B
    input  logic        inCarry,   // Initial carry-in
    output logic [15:0] outSum,    // 16-bit sum output
    output logic        outCarry   // Final carry-out
);

    // -------------------------------------------------------
    // Internal Signals
    // -------------------------------------------------------
    logic [7:0] sum_lower;
    logic       carry_from_lower;

    logic [7:0] sum_upper_c0;
    logic       carry_upper_c0;

    logic [7:0] sum_upper_c1;
    logic       carry_upper_c1;

    // -------------------------------------------------------
    // Lower 8-bit carry select adder (always computed)
    // -------------------------------------------------------
    adder_cs_8bit u_lower (
        .inA(inA[7:0]),
        .inB(inB[7:0]),
        .inCarry(inCarry),
        .outSum(sum_lower),
        .outCarry(carry_from_lower)
    );

    // -------------------------------------------------------
    // Upper 8-bit carry select adder assuming carry=0
    // -------------------------------------------------------
    adder_cs_8bit u_upper_c0 (
        .inA(inA[15:8]),
        .inB(inB[15:8]),
        .inCarry(1'b0),
        .outSum(sum_upper_c0),
        .outCarry(carry_upper_c0)
    );

    // -------------------------------------------------------
    // Upper 8-bit carry select adder assuming carry=1
    // -------------------------------------------------------
    adder_cs_8bit u_upper_c1 (
        .inA(inA[15:8]),
        .inB(inB[15:8]),
        .inCarry(1'b1),
        .outSum(sum_upper_c1),
        .outCarry(carry_upper_c1)
    );

    // -------------------------------------------------------
    // Multiplexers to select correct upper result
    // -------------------------------------------------------
    mux_2to1_8bit u_sum_mux (
        .in0(sum_upper_c0),         // Selected when carry_from_lower = 0
        .in1(sum_upper_c1),         // Selected when carry_from_lower = 1
        .sel(carry_from_lower),
        .out(outSum[15:8])
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
    assign outSum[7:0] = sum_lower;

endmodule