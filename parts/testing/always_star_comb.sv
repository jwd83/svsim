module always_star_comb (
    input  [7:0] a,
    input  [7:0] b,
    input        sel,
    output [7:0] out
);

    // Verilog-2001 style combinational block using @*
    always @* begin
        if (sel)
            out = a;
        else
            out = b;
    end

endmodule
