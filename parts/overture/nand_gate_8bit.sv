// ------------------- 8-Bit NAND Gate -------------------
module nand_gate_8bit (
    input  logic [7:0] inA,
    input  logic [7:0] inB,
    output logic [7:0] outY
);
    nand_gate u_nand0 (.inA(inA[0]), .inB(inB[0]), .outY(outY[0]));
    nand_gate u_nand1 (.inA(inA[1]), .inB(inB[1]), .outY(outY[1]));
    nand_gate u_nand2 (.inA(inA[2]), .inB(inB[2]), .outY(outY[2]));
    nand_gate u_nand3 (.inA(inA[3]), .inB(inB[3]), .outY(outY[3]));
    nand_gate u_nand4 (.inA(inA[4]), .inB(inB[4]), .outY(outY[4]));
    nand_gate u_nand5 (.inA(inA[5]), .inB(inB[5]), .outY(outY[5]));
    nand_gate u_nand6 (.inA(inA[6]), .inB(inB[6]), .outY(outY[6]));
    nand_gate u_nand7 (.inA(inA[7]), .inB(inB[7]), .outY(outY[7]));
endmodule