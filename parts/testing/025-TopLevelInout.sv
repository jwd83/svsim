// A single-bit shared bus exposed directly through a top-level `inout` port.
// One internal driver (gated by `internal_en`) competes with whatever the
// JSON harness drives onto `bus` — either a real value or `1'bz` to release.

module top(
    input wire internal_en,
    input wire internal_value,
    inout wire bus
);
    assign bus = internal_en ? internal_value : 1'bz;
endmodule
