// ------------------- XNOR Gate Using NAND -------------------
// Function: Output is 1 if inputs are the same
// Truth table:
// inA | inB | outY
//  0  |  0  |  1
//  0  |  1  |  0
//  1  |  0  |  0
//  1  |  1  |  1
module xnor_gate (
    input  logic inA,
    input  logic inB,
    output logic outY
);
    logic xor_out;

    // Step 1: XOR gate built from NANDs
    xor_gate u_xor (.inA(inA), .inB(inB), .outY(xor_out));

    // Step 2: NOT the XOR to get XNOR
    not_gate u_not (.inA(xor_out), .outY(outY));
endmodule
