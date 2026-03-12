// ===========================================================
// 3-to-8 Line Decoder
// ===========================================================
// Inputs: addr[2:0]
// Output: out[7:0] (one-hot)
//
// Decodes a 3-bit address to one-hot output:
// addr=0 -> out=00000001, addr=1 -> out=00000010, etc.

module decoder_3to8 (
    input  logic [2:0] addr,
    output logic [7:0] out
);
    // Invert each address bit
    logic n_addr0, n_addr1, n_addr2;

    not_gate u_inv0 (.inA(addr[0]), .outY(n_addr0));
    not_gate u_inv1 (.inA(addr[1]), .outY(n_addr1));
    not_gate u_inv2 (.inA(addr[2]), .outY(n_addr2));

    // First stage: decode addr[1:0] into four pair signals
    logic pair_00, pair_01, pair_10, pair_11;

    and_gate u_pair0 (.inA(n_addr1), .inB(n_addr0), .outY(pair_00));  // addr[1:0]=00
    and_gate u_pair1 (.inA(n_addr1), .inB(addr[0]), .outY(pair_01));  // addr[1:0]=01
    and_gate u_pair2 (.inA(addr[1]), .inB(n_addr0), .outY(pair_10));  // addr[1:0]=10
    and_gate u_pair3 (.inA(addr[1]), .inB(addr[0]), .outY(pair_11));  // addr[1:0]=11

    // Second stage: combine with addr[2] to produce 8 one-hot outputs
    and_gate u_out0 (.inA(n_addr2), .inB(pair_00), .outY(out[0]));  // 000
    and_gate u_out1 (.inA(n_addr2), .inB(pair_01), .outY(out[1]));  // 001
    and_gate u_out2 (.inA(n_addr2), .inB(pair_10), .outY(out[2]));  // 010
    and_gate u_out3 (.inA(n_addr2), .inB(pair_11), .outY(out[3]));  // 011
    and_gate u_out4 (.inA(addr[2]), .inB(pair_00), .outY(out[4]));  // 100
    and_gate u_out5 (.inA(addr[2]), .inB(pair_01), .outY(out[5]));  // 101
    and_gate u_out6 (.inA(addr[2]), .inB(pair_10), .outY(out[6]));  // 110
    and_gate u_out7 (.inA(addr[2]), .inB(pair_11), .outY(out[7]));  // 111

endmodule
