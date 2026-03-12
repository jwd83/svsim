// ------------------- NOR Gate Using NAND -------------------
// Function: Output is inverted OR
// Truth table:
// inA | inB | outY
//  0  |  0  |  1
//  0  |  1  |  0
//  1  |  0  |  0
//  1  |  1  |  0
module nor_gate (
    input  logic inA,
    input  logic inB,
    output logic outY
);
    logic or_out;

    // Step 1: Use OR gate built from NANDs
    or_gate u_or (
        .inA(inA),
        .inB(inB),
        .outY(or_out)
    );

    // Step 2: NOT the OR result
    not_gate u_not (.inA(or_out), .outY(outY));
endmodule
