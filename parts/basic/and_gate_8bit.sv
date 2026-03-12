// ------------------- 8-Bit AND Gate -------------------
module and_gate_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    output logic [7:0] outY
);
    and_gate u_and0 (.inA(inA[0]), .inB(inB[0]), .outY(outY[0]));
    and_gate u_and1 (.inA(inA[1]), .inB(inB[1]), .outY(outY[1]));
    and_gate u_and2 (.inA(inA[2]), .inB(inB[2]), .outY(outY[2]));
    and_gate u_and3 (.inA(inA[3]), .inB(inB[3]), .outY(outY[3]));
    and_gate u_and4 (.inA(inA[4]), .inB(inB[4]), .outY(outY[4]));
    and_gate u_and5 (.inA(inA[5]), .inB(inB[5]), .outY(outY[5]));
    and_gate u_and6 (.inA(inA[6]), .inB(inB[6]), .outY(outY[6]));
    and_gate u_and7 (.inA(inA[7]), .inB(inB[7]), .outY(outY[7]));
endmodule