// ------------------- NAND Gate -------------------
// Function: Output is inverted AND
// Truth table:
// inA | inB | outY
//  0  |  0  |  1
//  0  |  1  |  1
//  1  |  0  |  1
//  1  |  1  |  0
module nand_gate (
    input  logic inA,
    input  logic inB,
    output logic outY
);
    assign outY = ~(inA & inB);
endmodule