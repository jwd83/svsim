// ===========================================================
// Half Adder using NAND-based Gates
// ===========================================================
// Inputs: inA, inB
// Outputs: outSum (A XOR B), outCarry (A AND B)
// Truth table:
// inA | inB | outSum | outCarry
//  0  |  0  |    0   |    0
//  0  |  1  |    1   |    0
//  1  |  0  |    1   |    0
//  1  |  1  |    0   |    1
module half_adder (
    input  logic inA,
    input  logic inB,
    output logic outSum,
    output logic outCarry
);
    // Sum is XOR of inputs
    xor_gate u_xor (
        .inA(inA),
        .inB(inB),
        .outY(outSum)
    );

    // Carry is AND of inputs
    and_gate u_and (
        .inA(inA),
        .inB(inB),
        .outY(outCarry)
    );
endmodule