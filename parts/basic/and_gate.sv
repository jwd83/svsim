// ------------------- AND Gate Using NAND -------------------
// Function: Output is 1 only if both inputs are 1
// Truth table:
// inA | inB | outY
//  0  |  0  |  0
//  0  |  1  |  0
//  1  |  0  |  0
//  1  |  1  |  1
module and_gate (
    input  logic inA,
    input  logic inB,
    output logic outY
);
    logic nand_out;

    // Step 1: NAND of inputs
    nand_gate u_nand1 (
        .inA(inA),
        .inB(inB),
        .outY(nand_out)
    );

    // Step 2: NOT the NAND result to get AND
    not_gate u_not (
        .inA(nand_out),
        .outY(outY)
    );
endmodule