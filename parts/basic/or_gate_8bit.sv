// ------------------- 8-Bit OR Gate -------------------
module or_gate_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    output logic [7:0] outY
);
    or_gate u_or0 (.inA(inA[0]), .inB(inB[0]), .outY(outY[0]));
    or_gate u_or1 (.inA(inA[1]), .inB(inB[1]), .outY(outY[1]));
    or_gate u_or2 (.inA(inA[2]), .inB(inB[2]), .outY(outY[2]));
    or_gate u_or3 (.inA(inA[3]), .inB(inB[3]), .outY(outY[3]));
    or_gate u_or4 (.inA(inA[4]), .inB(inB[4]), .outY(outY[4]));
    or_gate u_or5 (.inA(inA[5]), .inB(inB[5]), .outY(outY[5]));
    or_gate u_or6 (.inA(inA[6]), .inB(inB[6]), .outY(outY[6]));
    or_gate u_or7 (.inA(inA[7]), .inB(inB[7]), .outY(outY[7]));
endmodule