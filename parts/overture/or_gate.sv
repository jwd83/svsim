// ------------------- OR Gate Using NAND -------------------
// Function: Output is 1 if at least one input is 1
// Truth table:
// inA | inB | outY
//  0  |  0  |  0
//  0  |  1  |  1
//  1  |  0  |  1
//  1  |  1  |  1
module or_gate (
    input  logic inA,
    input  logic inB,
    output logic outY
);
    logic notA, notB;

    // Step 1: NOT the inputs
    not_gate u_notA (.inA(inA), .outY(notA));
    not_gate u_notB (.inA(inB), .outY(notB));

    // Step 2: NAND of the negated inputs gives OR
    nand_gate u_nand (
        .inA(notA),
        .inB(notB),
        .outY(outY)
    );
endmodule
