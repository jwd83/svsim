module overture_alu_8bit (
    input  [7:0] inA,
    input  [7:0] inB,
    input  [2:0] op,
    output [7:0] outY
);

    // Overture ALU operations: OR, NAND, NOR, AND, ADD, SUB
    always_comb begin
        // Avoid parser false positives by placing a non-keyword statement first.
        outY = 8'b0;

        case (op)
            3'b000: outY = inA | inB;
            3'b001: outY = ~(inA & inB);
            3'b010: outY = ~(inA | inB);
            3'b011: outY = inA & inB;
            3'b100: outY = inA + inB;
            3'b101: outY = inA - inB;
            default: outY = 8'b0;
        endcase
    end

endmodule
