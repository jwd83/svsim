module memory_cpu_stub (
    input clk,
    input reset,
    input run,
    output [3:0] pc,
    output [7:0] acc,
    output [7:0] ram_out
);

    reg [7:0] rom [15:0];
    reg [7:0] ram [15:0];
    reg [7:0] instr;

    always_ff @(posedge clk) begin
        if (reset) begin
            pc <= 0;
            acc <= 0;
            instr <= 0;
        end else if (run) begin
            instr = rom[pc];
            case (instr[7:6])
                2'b00: acc <= instr[5:0];
                2'b01: acc <= acc + instr[5:0];
                2'b10: ram[instr[3:0]] <= acc;
                default: acc <= acc;
            endcase
            pc <= pc + 1;
        end
    end

    assign ram_out = ram[0];

endmodule
