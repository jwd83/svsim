module duplicate_instance_child (
    output logic outY
);
    assign outY = 1'b1;
endmodule

module duplicate_instance_names (
    output logic outA,
    output logic outB
);
    duplicate_instance_child u_dup (
        .outY(outA)
    );

    duplicate_instance_child u_dup (
        .outY(outB)
    );
endmodule
