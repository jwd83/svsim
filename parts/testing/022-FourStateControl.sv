module top(
    output logic eq_guard,
    output logic not_guard,
    output logic case_default,
    output logic ternary_guard,
    output logic lt_guard
);
    wire maybe;
    logic [1:0] merged;

    assign maybe = 1'b0;
    assign maybe = 1'b1;
    assign merged = maybe ? 2'b10 : 2'b11;

    always_comb begin
        if (maybe == 1'b0) eq_guard = 1'b1;
        else eq_guard = 1'b0;

        if (!maybe) not_guard = 1'b1;
        else not_guard = 1'b0;

        if (merged[0]) ternary_guard = 1'b1;
        else ternary_guard = 1'b0;

        if (maybe < 1'b1) lt_guard = 1'b1;
        else lt_guard = 1'b0;

        case (maybe)
            1'b0: case_default = 1'b0;
            1'b1: case_default = 1'b0;
            default: case_default = 1'b1;
        endcase
    end
endmodule
