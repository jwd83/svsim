module overture_cpu_8bit_math_sequence (
    input clk,
    input reset,
    input run,
    output [7:0] pc,
    output [7:0] acc,
    output halted
);

    // Overture-style toy program:
    //   PC 0: LOADI ACC, 3
    //   PC 1: ADDI  ACC, 5
    //   PC 2: ADDI  ACC, 2
    //   PC 3: HALT

    // Program steps
    always_ff @(posedge clk) begin
        if (pc == 0) begin
            acc <= 3;
        end
    end

    always_ff @(posedge clk) begin
        if (pc == 1) begin
            acc <= acc + 5;
        end
    end

    always_ff @(posedge clk) begin
        if (pc == 2) begin
            acc <= acc + 2;
        end
    end

    always_ff @(posedge clk) begin
        if (pc == 3) begin
            halted <= 1;
        end
    end

    // Pause/hold behavior when run is low
    always_ff @(posedge clk) begin
        if (run == 0) begin
            acc <= acc;
        end
    end

    always_ff @(posedge clk) begin
        if (run == 0) begin
            halted <= halted;
        end
    end

    // PC control
    always_ff @(posedge clk) begin
        if (run) begin
            pc <= pc + 1;
        end
    end

    always_ff @(posedge clk) begin
        if (run == 0) begin
            pc <= pc;
        end
    end

    // Hold PC once halted (based on previous cycle halted state)
    always_ff @(posedge clk) begin
        if (halted == 1) begin
            pc <= pc;
        end
    end

    // Reset behavior
    always_ff @(posedge clk) begin
        if (reset) begin
            acc <= 0;
        end
    end

    always_ff @(posedge clk) begin
        if (reset) begin
            pc <= 0;
        end
    end

    always_ff @(posedge clk) begin
        if (reset) begin
            halted <= 0;
        end
    end

endmodule
