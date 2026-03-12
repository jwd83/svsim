// ------------------- 8-Bit NOR Gate -------------------
module nor_gate_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    output logic [7:0] outY
);
    nor_gate u_nor0 (.inA(inA[0]), .inB(inB[0]), .outY(outY[0]));
    nor_gate u_nor1 (.inA(inA[1]), .inB(inB[1]), .outY(outY[1]));
    nor_gate u_nor2 (.inA(inA[2]), .inB(inB[2]), .outY(outY[2]));
    nor_gate u_nor3 (.inA(inA[3]), .inB(inB[3]), .outY(outY[3]));
    nor_gate u_nor4 (.inA(inA[4]), .inB(inB[4]), .outY(outY[4]));
    nor_gate u_nor5 (.inA(inA[5]), .inB(inB[5]), .outY(outY[5]));
    nor_gate u_nor6 (.inA(inA[6]), .inB(inB[6]), .outY(outY[6]));
    nor_gate u_nor7 (.inA(inA[7]), .inB(inB[7]), .outY(outY[7]));
endmodule
