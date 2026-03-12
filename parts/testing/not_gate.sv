// ------------------- NOT Gate Using NAND -------------------
// Function: Inverts a single input
// Truth table:
// inA | outY
//  0  |  1
//  1  |  0
module not_gate (
    input  logic inA,
    output logic outY
);
    // A NOT gate is simply a NAND gate with both inputs tied together
    nand_gate u_nand (
        .inA(inA),
        .inB(inA),
        .outY(outY)
    );
endmodule