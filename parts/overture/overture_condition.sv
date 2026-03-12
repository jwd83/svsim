module overture_condition (
    input  [7:0] r3,
    input  [2:0] cond_sel,
    output       cond_met
);

    always_comb begin
        cond_met = 1'b0;

        case (cond_sel)
            3'b000: cond_met = 1'b0;                              // never
            3'b001: cond_met = (r3 == 8'b0);                      // eq
            3'b010: cond_met = r3[7];                             // less
            3'b011: cond_met = r3[7] || (r3 == 8'b0);            // less_eq
            3'b100: cond_met = 1'b1;                              // always
            3'b101: cond_met = (r3 != 8'b0);                      // not_eq
            3'b110: cond_met = (r3[7] == 1'b0);                  // greater_eq
            default: cond_met = (r3[7] == 1'b0) && (r3 != 8'b0); // greater
        endcase
    end

endmodule
