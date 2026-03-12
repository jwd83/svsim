// ------------------- 8-Bit XNOR Gate -------------------
module xnor_gate_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    output logic [7:0] outY
);
    xnor_gate u_xnor0 (.inA(inA[0]), .inB(inB[0]), .outY(outY[0]));
    xnor_gate u_xnor1 (.inA(inA[1]), .inB(inB[1]), .outY(outY[1]));
    xnor_gate u_xnor2 (.inA(inA[2]), .inB(inB[2]), .outY(outY[2]));
    xnor_gate u_xnor3 (.inA(inA[3]), .inB(inB[3]), .outY(outY[3]));
    xnor_gate u_xnor4 (.inA(inA[4]), .inB(inB[4]), .outY(outY[4]));
    xnor_gate u_xnor5 (.inA(inA[5]), .inB(inB[5]), .outY(outY[5]));
    xnor_gate u_xnor6 (.inA(inA[6]), .inB(inB[6]), .outY(outY[6]));
    xnor_gate u_xnor7 (.inA(inA[7]), .inB(inB[7]), .outY(outY[7]));
endmodule
