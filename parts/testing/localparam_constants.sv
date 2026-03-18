module localparam_constants (
    input  [7:0] sel,
    output [7:0] y
);
    localparam [7:0] STATE_IDLE  = 8'd0;
    localparam [7:0] STATE_RUN   = 8'd1;
    localparam [7:0] STATE_DONE  = 8'd2;
    localparam [7:0] STATE_ERROR = 8'd3;

    reg [7:0] out;
    always_comb begin
        case (sel)
            STATE_IDLE:  out = 8'hAA;
            STATE_RUN:   out = 8'hBB;
            STATE_DONE:  out = 8'hCC;
            STATE_ERROR: out = 8'hDD;
            default:     out = 8'hFF;
        endcase
    end
    assign y = out;
endmodule
