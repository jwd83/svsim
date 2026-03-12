// ------------------- 8-Bit XOR Gate -------------------
module xor_gate_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    output logic [7:0] outY
);
    xor_gate u_xor0 (.inA(inA[0]), .inB(inB[0]), .outY(outY[0]));
    xor_gate u_xor1 (.inA(inA[1]), .inB(inB[1]), .outY(outY[1]));
    xor_gate u_xor2 (.inA(inA[2]), .inB(inB[2]), .outY(outY[2]));
    xor_gate u_xor3 (.inA(inA[3]), .inB(inB[3]), .outY(outY[3]));
    xor_gate u_xor4 (.inA(inA[4]), .inB(inB[4]), .outY(outY[4]));
    xor_gate u_xor5 (.inA(inA[5]), .inB(inB[5]), .outY(outY[5]));
    xor_gate u_xor6 (.inA(inA[6]), .inB(inB[6]), .outY(outY[6]));
    xor_gate u_xor7 (.inA(inA[7]), .inB(inB[7]), .outY(outY[7]));
endmodule
