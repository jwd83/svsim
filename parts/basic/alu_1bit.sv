module alu_1bit (
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic [1:0] op,
    output logic [7:0] out
);

    always_comb begin
        if (op == 2'b00)
            out = a & b;
        else if (op == 2'b01)
            out = a | b;
        else if (op == 2'b10)
            out = a ^ b;
        else
            out = ~a;
    end

endmodule
