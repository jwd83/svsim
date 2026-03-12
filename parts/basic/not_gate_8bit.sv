// ------------------- 8-Bit NOT Gate -------------------
module not_gate_8bit (
    input  logic [7:0] inA,
    output logic [7:0] outY
);
    not_gate u_not0 (.inA(inA[0]), .outY(outY[0]));
    not_gate u_not1 (.inA(inA[1]), .outY(outY[1]));
    not_gate u_not2 (.inA(inA[2]), .outY(outY[2]));
    not_gate u_not3 (.inA(inA[3]), .outY(outY[3]));
    not_gate u_not4 (.inA(inA[4]), .outY(outY[4]));
    not_gate u_not5 (.inA(inA[5]), .outY(outY[5]));
    not_gate u_not6 (.inA(inA[6]), .outY(outY[6]));
    not_gate u_not7 (.inA(inA[7]), .outY(outY[7]));
endmodule
