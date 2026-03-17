module missing_child_module (
    output logic outY
);
    missing_dependency u_missing (
        .outY(outY)
    );
endmodule
