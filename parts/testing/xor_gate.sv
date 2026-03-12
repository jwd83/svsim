// ------------------- XOR Gate Using NAND -------------------
// Function: Output is 1 if inputs are different
// Truth table:
// inA | inB | outY
//  0  |  0  |  0
//  0  |  1  |  1
//  1  |  0  |  1
//  1  |  1  |  0
module xor_gate (
    input  logic inA,
    input  logic inB,
    output logic outY
);
    logic nand1_out, nand2_out, nand3_out, nand4_out;

    // Step 1: NAND of the inputs
    nand_gate u_nand1 (.inA(inA), .inB(inB), .outY(nand1_out));

    // Step 2: NAND of inA and nand1_out
    nand_gate u_nand2 (.inA(inA), .inB(nand1_out), .outY(nand2_out));

    // Step 3: NAND of inB and nand1_out
    nand_gate u_nand3 (.inA(inB), .inB(nand1_out), .outY(nand3_out));

    // Step 4: NAND of nand2_out and nand3_out gives XOR
    nand_gate u_nand4 (.inA(nand2_out), .inB(nand3_out), .outY(outY));
endmodule
