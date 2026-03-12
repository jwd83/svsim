module mux_4to1_comb (
    input  logic [7:0] d0,
    input  logic [7:0] d1,
    input  logic [7:0] d2,
    input  logic [7:0] d3,
    input  logic [1:0] sel,
    output logic [7:0] out
);

    always_comb begin
        case (sel)
            2'b00: out = d0;
            2'b01: out = d1;
            2'b10: out = d2;
            2'b11: out = d3;
            default: out = 8'h00;
        endcase
    end

endmodule
