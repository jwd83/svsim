// ===========================================================
// 2:1 Multiplexer (1-bit)
// ===========================================================
// Inputs: in0, in1, sel
// Output: out
//
// Truth table:
// sel | out
// ----+----
//  0  | in0
//  1  | in1
//
// Logic: out = (sel & in1) | (~sel & in0)

module mux_2to1_1bit (
    input  logic in0,   // Input 0
    input  logic in1,   // Input 1  
    input  logic sel,   // Select signal
    output logic out    // Output
);
    // Intermediate signals for clarity
    logic sel_n;        // Inverted select
    logic and0, and1;   // AND gate outputs
    
    // Invert select signal
    not_gate u_not_sel (.inA(sel), .outY(sel_n));
    
    // AND gates for each input path
    and_gate u_and0 (.inA(sel_n), .inB(in0), .outY(and0));
    and_gate u_and1 (.inA(sel), .inB(in1), .outY(and1));
    
    // OR gate to combine paths
    or_gate u_or (.inA(and0), .inB(and1), .outY(out));

endmodule