use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::SimulationSession;
use crate::{BitValue, Compiler, LogicValue};

fn lv(value: u64) -> LogicValue {
    LogicValue::from(value)
}

fn inputs<const N: usize>(pairs: [(String, u64); N]) -> BTreeMap<String, LogicValue> {
    pairs
        .into_iter()
        .map(|(name, value)| (name, lv(value)))
        .collect()
}

fn words<const N: usize>(values: [u64; N]) -> Vec<LogicValue> {
    values.into_iter().map(lv).collect()
}

fn step_posedge<const N: usize>(
    sim: &mut SimulationSession,
    pairs: [(String, u64); N],
) -> BTreeMap<String, LogicValue> {
    let mut low_inputs = inputs(pairs.clone());
    low_inputs.insert("clk".into(), lv(0));
    sim.step(low_inputs).expect("step low");

    let mut high_inputs = inputs(pairs);
    high_inputs.insert("clk".into(), lv(1));
    sim.step(high_inputs).expect("step high")
}

fn persisted_u64(sim: &super::SimulationSession, state: &super::ModuleState, name: &str) -> u64 {
    let binding = state
        .signals
        .get(name)
        .copied()
        .unwrap_or_else(|| panic!("missing signal binding '{name}'"));
    super::read_binding(binding, &sim.persisted)
        .expect("read persisted binding")
        .to_bit_value_checked()
        .expect("persisted value is 2-state")
        .to_u64_checked()
        .expect("persisted value fits in u64")
}

fn eval_once_logic<const N: usize>(
    sim: &mut SimulationSession,
    pairs: [(String, u64); N],
) -> BTreeMap<String, LogicValue> {
    sim.eval_once(inputs(pairs)).expect("eval")
}

fn memory_u64(state: &super::ModuleState, name: &str, index: usize) -> u64 {
    state
        .memories
        .get(name)
        .unwrap_or_else(|| panic!("missing memory '{name}'"))
        .read(index, name)
        .expect("read memory")
        .to_bit_value_checked()
        .expect("memory value is 2-state")
        .to_u64_checked()
        .expect("memory value fits in u64")
}

fn child_state<'a>(state: &'a super::ModuleState, name: &str) -> &'a super::ChildState {
    state
        .children
        .iter()
        .find(|child| child.instance_name == name)
        .unwrap_or_else(|| panic!("missing child instance '{name}'"))
}

macro_rules! assert_signal_eq {
    ($outputs:expr, $name:expr, $value:expr) => {
        assert_eq!(
            $outputs
                .get($name)
                .and_then(|value| value.to_bit_value_checked()),
            Some(BitValue::from($value as u64))
        );
    };
}

macro_rules! assert_logic_eq {
    ($actual:expr, $value:expr) => {
        assert_eq!(
            ($actual).to_bit_value_checked(),
            Some(BitValue::from($value as u64))
        );
    };
}

macro_rules! assert_logic_bits_eq {
    ($actual:expr, $value:expr) => {
        assert_eq!(($actual).to_bit_value_checked(), Some($value));
    };
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

#[test]
fn eval_once_runs_leaf_assign_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/nand_gate.sv"))
        .expect("compile nand gate");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([("inA".into(), 1), ("inB".into(), 1)]))
        .expect("eval");

    assert_signal_eq!(outputs, "outY", 0);
}

#[test]
fn eval_once_runs_hierarchical_combinational_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/full_adder.sv"))
        .expect("compile full adder");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("inA".into(), 1),
            ("inB".into(), 1),
            ("inCarry".into(), 1),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "outSum", 1);
    assert_signal_eq!(outputs, "outCarry", 1);
}

#[test]
fn structural_runtime_shares_sibling_net_bindings() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module producer(output wire y); ",
                "assign y = 1'b1; ",
                "endmodule\n",
                "module consumer(input wire a, output logic y); ",
                "assign y = a; ",
                "endmodule\n",
                "module top(output logic out); ",
                "wire link; ",
                "producer u_prod(.y(link)); ",
                "consumer u_cons(.a(link), .y(out)); ",
                "endmodule\n"
            ),
        )
        .expect("compile structural fixture");
    let mut sim = design.instantiate_top().expect("instantiate");

    let top_link = sim
        .state
        .signals
        .get("link")
        .copied()
        .expect("top link binding");
    let producer_y = child_state(&sim.state, "u_prod")
        .state
        .signals
        .get("y")
        .copied()
        .expect("producer y binding");
    let consumer_a = child_state(&sim.state, "u_cons")
        .state
        .signals
        .get("a")
        .copied()
        .expect("consumer a binding");

    assert_eq!(top_link.object_id, producer_y.object_id);
    assert_eq!(top_link.object_id, consumer_a.object_id);

    let outputs = sim.eval_once(BTreeMap::new()).expect("eval");
    assert_signal_eq!(outputs, "out", 1);
}

#[test]
fn eval_once_logic_reports_floating_tri_outputs() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            "module top(output tri out); endmodule\n",
        )
        .expect("compile floating tri");
    let mut sim = design.instantiate_top().expect("instantiate");

    let logic_outputs = eval_once_logic(&mut sim, []);
    assert_eq!(
        logic_outputs.get("out"),
        Some(&LogicValue::from_logic_str("z").expect("z logic"))
    );

    let error = sim
        .eval_once_2state(BTreeMap::new())
        .expect_err("2-state wrapper should reject z");
    assert!(
        error
            .to_string()
            .contains("resolved to four-state value 'z'"),
        "unexpected error: {error}"
    );
}

#[test]
fn eval_once_applies_tri1_pull_default_through_resolver() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            "module top(output tri1 out); endmodule\n",
        )
        .expect("compile tri1");
    let mut sim = design.instantiate_top().expect("instantiate");

    let logic_outputs = eval_once_logic(&mut sim, []);
    assert_eq!(
        logic_outputs.get("out"),
        Some(&LogicValue::from_logic_str("1").expect("one logic"))
    );

    let outputs = sim.eval_once(BTreeMap::new()).expect("eval tri1");
    assert_signal_eq!(outputs, "out", 1);
}

#[test]
fn eval_once_resolves_conflicting_wire_drivers_to_x() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(output wire out); ",
                "assign out = 1'b0; ",
                "assign out = 1'b1; ",
                "endmodule\n"
            ),
        )
        .expect("compile conflicting wire");
    let mut sim = design.instantiate_top().expect("instantiate");

    let logic_outputs = eval_once_logic(&mut sim, []);
    assert_eq!(
        logic_outputs.get("out"),
        Some(&LogicValue::from_logic_str("x").expect("x logic"))
    );

    let error = sim
        .eval_once_2state(BTreeMap::new())
        .expect_err("2-state wrapper should reject x");
    assert!(
        error
            .to_string()
            .contains("resolved to four-state value 'x'"),
        "unexpected error: {error}"
    );
}

#[test]
fn eval_once_logic_merges_unknown_ternary_condition_bits() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(output logic [1:0] merged); ",
                "wire maybe; ",
                "assign maybe = 1'b0; ",
                "assign maybe = 1'b1; ",
                "assign merged = maybe ? 2'b10 : 2'b11; ",
                "endmodule\n"
            ),
        )
        .expect("compile ternary merge");
    let mut sim = design.instantiate_top().expect("instantiate");

    let logic_outputs = eval_once_logic(&mut sim, []);
    assert_eq!(
        logic_outputs.get("merged"),
        Some(&LogicValue::from_logic_str("1x").expect("1x logic"))
    );

    let error = sim
        .eval_once_2state(BTreeMap::new())
        .expect_err("2-state wrapper should reject merged x");
    assert!(
        error
            .to_string()
            .contains("resolved to four-state value '1x'"),
        "unexpected error: {error}"
    );
}

#[test]
fn eval_once_applies_four_state_control_semantics_after_net_resolution() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "output logic eq_guard, ",
                "output logic not_guard, ",
                "output logic case_default, ",
                "output logic ternary_guard, ",
                "output logic lt_guard",
                "); ",
                "wire maybe; ",
                "logic [1:0] merged; ",
                "assign maybe = 1'b0; ",
                "assign maybe = 1'b1; ",
                "assign merged = maybe ? 2'b10 : 2'b11; ",
                "always_comb begin ",
                "  if (maybe == 1'b0) eq_guard = 1'b1; else eq_guard = 1'b0; ",
                "  if (!maybe) not_guard = 1'b1; else not_guard = 1'b0; ",
                "  if (merged[0]) ternary_guard = 1'b1; else ternary_guard = 1'b0; ",
                "  if (maybe < 1'b1) lt_guard = 1'b1; else lt_guard = 1'b0; ",
                "  case (maybe) ",
                "    1'b0: case_default = 1'b0; ",
                "    1'b1: case_default = 1'b0; ",
                "    default: case_default = 1'b1; ",
                "  endcase ",
                "end ",
                "endmodule\n"
            ),
        )
        .expect("compile four-state control");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim.eval_once(BTreeMap::new()).expect("eval");
    assert_signal_eq!(outputs, "eq_guard", 0);
    assert_signal_eq!(outputs, "not_guard", 0);
    assert_signal_eq!(outputs, "case_default", 1);
    assert_signal_eq!(outputs, "ternary_guard", 0);
    assert_signal_eq!(outputs, "lt_guard", 0);
}

#[test]
fn eval_once_applies_wand_resolution_in_settle_path() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(output wand out); ",
                "assign out = 1'b1; ",
                "assign out = 1'b0; ",
                "endmodule\n"
            ),
        )
        .expect("compile wand");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim.eval_once(BTreeMap::new()).expect("eval wand");
    assert_signal_eq!(outputs, "out", 0);
}

#[test]
fn eval_once_rejects_multiple_active_uwire_drivers() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(output uwire out); ",
                "assign out = 1'b0; ",
                "assign out = 1'b1; ",
                "endmodule\n"
            ),
        )
        .expect("compile uwire");
    let mut sim = design.instantiate_top().expect("instantiate");

    let error = sim
        .eval_once(BTreeMap::new())
        .expect_err("uwire should reject contention");
    assert!(
        error.to_string().contains("multiple active drivers"),
        "unexpected error: {error}"
    );
}

#[test]
fn structural_runtime_aliases_input_bindings_across_width_changes() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module pass4(input wire [3:0] a, output logic [3:0] y); ",
                "assign y = a; ",
                "endmodule\n",
                "module top(input logic in, output logic [3:0] out); ",
                "pass4 u_pass(.a(in), .y(out)); ",
                "endmodule\n"
            ),
        )
        .expect("compile width alias fixture");
    let mut sim = design.instantiate_top().expect("instantiate");

    let top_in = sim
        .state
        .signals
        .get("in")
        .copied()
        .expect("top input binding");
    let child_a = child_state(&sim.state, "u_pass")
        .state
        .signals
        .get("a")
        .copied()
        .expect("child input binding");

    assert_eq!(top_in.object_id, child_a.object_id);
    assert_eq!(top_in.view_width, 1);
    assert_eq!(child_a.view_width, 4);

    let outputs = sim
        .eval_once(inputs([("in".into(), 1)]))
        .expect("eval width alias");
    assert_signal_eq!(outputs, "out", 0b0001);
}

#[test]
fn structural_runtime_aliases_internal_inout_bindings_to_parent_nets() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module bus_driver(",
                "input logic en, ",
                "input logic [3:0] value, ",
                "inout wire [3:0] bus",
                "); ",
                "wire [3:0] float_bus; ",
                "assign bus = en ? value : float_bus; ",
                "endmodule\n",
                "module top(",
                "input logic en, ",
                "input logic [3:0] value, ",
                "output wire [3:0] out",
                "); ",
                "wire [3:0] bus; ",
                "bus_driver u_driver(.en(en), .value(value), .bus(bus)); ",
                "assign out = bus; ",
                "endmodule\n"
            ),
        )
        .expect("compile internal inout alias fixture");
    let mut sim = design.instantiate_top().expect("instantiate");

    let top_bus = sim
        .state
        .signals
        .get("bus")
        .copied()
        .expect("top bus binding");
    let child_bus = child_state(&sim.state, "u_driver")
        .state
        .signals
        .get("bus")
        .copied()
        .expect("child inout binding");

    assert_eq!(top_bus.object_id, child_bus.object_id);
    assert_eq!(top_bus.view_width, 4);
    assert_eq!(child_bus.view_width, 4);

    let floating = sim
        .eval_once(inputs([("en".into(), 0), ("value".into(), 0b1010)]))
        .expect("eval floating inout");
    assert_eq!(
        floating.get("out"),
        Some(&LogicValue::from_logic_str("zzzz").expect("zzzz logic"))
    );

    let driven = sim
        .eval_once(inputs([("en".into(), 1), ("value".into(), 0b1010)]))
        .expect("eval driven inout");
    assert_signal_eq!(driven, "out", 0b1010);
}

#[test]
fn eval_once_resolves_internal_inout_bus_contention_to_x() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module bus_driver(",
                "input logic en, ",
                "input logic value, ",
                "inout wire bus",
                "); ",
                "wire float_bus; ",
                "assign bus = en ? value : float_bus; ",
                "endmodule\n",
                "module top(",
                "input logic drive_low, ",
                "input logic drive_high, ",
                "output wire out",
                "); ",
                "wire bus; ",
                "bus_driver low(.en(drive_low), .value(1'b0), .bus(bus)); ",
                "bus_driver high(.en(drive_high), .value(1'b1), .bus(bus)); ",
                "assign out = bus; ",
                "endmodule\n"
            ),
        )
        .expect("compile internal inout contention");
    let mut sim = design.instantiate_top().expect("instantiate");

    let floating = sim
        .eval_once(inputs([("drive_low".into(), 0), ("drive_high".into(), 0)]))
        .expect("eval floating bus");
    assert_eq!(
        floating.get("out"),
        Some(&LogicValue::from_logic_str("z").expect("z logic"))
    );

    let low = sim
        .eval_once(inputs([("drive_low".into(), 1), ("drive_high".into(), 0)]))
        .expect("eval low bus");
    assert_signal_eq!(low, "out", 0);

    let high = sim
        .eval_once(inputs([("drive_low".into(), 0), ("drive_high".into(), 1)]))
        .expect("eval high bus");
    assert_signal_eq!(high, "out", 1);

    let conflicted = sim
        .eval_once(inputs([("drive_low".into(), 1), ("drive_high".into(), 1)]))
        .expect("eval conflicted bus");
    assert_eq!(
        conflicted.get("out"),
        Some(&LogicValue::from_logic_str("x").expect("x logic"))
    );
}

#[test]
fn structural_runtime_keeps_parent_bits_when_child_drives_part_select() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module upper(output logic [3:0] y); ",
                "assign y = 4'ha; ",
                "endmodule\n",
                "module top(output logic [7:0] out); ",
                "assign out[3:0] = 4'h5; ",
                "upper u_upper(.y(out[7:4])); ",
                "endmodule\n"
            ),
        )
        .expect("compile mixed parent/child part-select fixture");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(BTreeMap::new())
        .expect("eval mixed part-select");
    assert_signal_eq!(outputs, "out", 0xa5);
}

#[test]
fn eval_once_runs_vector_ternary_assign() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/ternary_mux.sv"))
        .expect("compile ternary mux");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("a".into(), 0x12),
            ("b".into(), 0x34),
            ("sel".into(), 1),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 0x12);
}

#[test]
fn eval_once_normalizes_ternary_width_before_concatenation() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic sel, ",
                "output logic [3:0] out",
                "); ",
                "assign out = {2'b10, (sel ? 2'b11 : 1'b1)}; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("sel".into(), 0)]))
        .expect("eval false branch");
    assert_signal_eq!(outputs, "out", 0b1001);

    let outputs = sim
        .eval_once(inputs([("sel".into(), 1)]))
        .expect("eval true branch");
    assert_signal_eq!(outputs, "out", 0b1011);
}

#[test]
fn eval_once_normalizes_ternary_width_before_replication() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic sel, ",
                "output logic [3:0] out",
                "); ",
                "assign out = {2{sel ? 2'b10 : 1'b1}}; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("sel".into(), 0)]))
        .expect("eval false branch");
    assert_signal_eq!(outputs, "out", 0b0101);

    let outputs = sim
        .eval_once(inputs([("sel".into(), 1)]))
        .expect("eval true branch");
    assert_signal_eq!(outputs, "out", 0b1010);
}

#[test]
fn eval_once_keeps_nested_ternary_false_branches_after_param_folding() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic [31:0] in, ",
                "output logic [31:0] out",
                "); ",
                "localparam logic A = 1'b0; ",
                "localparam logic B = 1'b0; ",
                "assign out = A ? 32'h11111111 : B ? 32'h22222222 : in; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("in".into(), 0x0010_0093)]))
        .expect("eval nested ternary");
    assert_signal_eq!(outputs, "out", 0x0010_0093);
}

#[test]
fn eval_once_gives_conditional_lower_precedence_than_logical_and() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic gate, ",
                "input logic [31:0] in, ",
                "output logic [31:0] out",
                "); ",
                "localparam logic A = 1'b0; ",
                "assign out = A && gate ? 32'h11111111 : in; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("gate".into(), 1), ("in".into(), 0x0010_0093)]))
        .expect("eval conditional precedence");
    assert_signal_eq!(outputs, "out", 0x0010_0093);
}

#[test]
fn eval_once_gives_equality_higher_precedence_than_logical_and() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic [31:0] in, ",
                "output logic is_shift_imm",
                "); ",
                "assign is_shift_imm = |{",
                "in[14:12] == 3'b001 && in[31:25] == 7'b0000000, ",
                "in[14:12] == 3'b101 && in[31:25] == 7'b0000000, ",
                "in[14:12] == 3'b101 && in[31:25] == 7'b0100000",
                "}; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("in".into(), 0x0010_0093)]))
        .expect("eval addi helper");
    assert_signal_eq!(outputs, "is_shift_imm", 0);

    let outputs = sim
        .eval_once(inputs([("in".into(), 0x0010_1093)]))
        .expect("eval slli helper");
    assert_signal_eq!(outputs, "is_shift_imm", 1);
}

#[test]
fn eval_once_coerces_assignment_and_instance_port_widths() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module pass4(",
                "input logic [3:0] in, ",
                "output logic [3:0] out",
                "); ",
                "assign out = in; ",
                "endmodule\n",
                "module pass2(",
                "input logic [1:0] in, ",
                "output logic [1:0] out",
                "); ",
                "assign out = in; ",
                "endmodule\n",
                "module bit_driver(",
                "output logic out",
                "); ",
                "assign out = 1'b1; ",
                "endmodule\n",
                "module bus_driver(",
                "output logic [4:0] out",
                "); ",
                "assign out = 5'b10101; ",
                "endmodule\n",
                "module top(",
                "input logic a, ",
                "input logic [7:0] wide_in, ",
                "output logic [3:0] assign_widened, ",
                "output logic [1:0] assign_narrowed, ",
                "output logic [3:0] child_input_widened, ",
                "output logic [1:0] child_input_narrowed, ",
                "output logic [5:0] child_output_widened, ",
                "output logic [2:0] child_output_narrowed",
                "); ",
                "assign assign_widened = a; ",
                "assign assign_narrowed = wide_in; ",
                "pass4 widen_input(.in(a), .out(child_input_widened)); ",
                "pass2 narrow_input(.in(wide_in), .out(child_input_narrowed)); ",
                "bit_driver widen_output(.out(child_output_widened)); ",
                "bus_driver narrow_output(.out(child_output_narrowed)); ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("a".into(), 1), ("wide_in".into(), 0xab)]))
        .expect("eval coercion case");
    assert_signal_eq!(outputs, "assign_widened", 0b0001);
    assert_signal_eq!(outputs, "assign_narrowed", 0b11);
    assert_signal_eq!(outputs, "child_input_widened", 0b0001);
    assert_signal_eq!(outputs, "child_input_narrowed", 0b11);
    assert_signal_eq!(outputs, "child_output_widened", 0b000001);
    assert_signal_eq!(outputs, "child_output_narrowed", 0b101);

    let outputs = sim
        .eval_once(inputs([("a".into(), 0), ("wide_in".into(), 0x04)]))
        .expect("eval truncated-away bits case");
    assert_signal_eq!(outputs, "assign_widened", 0);
    assert_signal_eq!(outputs, "assign_narrowed", 0);
    assert_signal_eq!(outputs, "child_input_widened", 0);
    assert_signal_eq!(outputs, "child_input_narrowed", 0);
    assert_signal_eq!(outputs, "child_output_widened", 0b000001);
    assert_signal_eq!(outputs, "child_output_narrowed", 0b101);
}

#[test]
fn eval_once_runs_shift_operators_with_left_operand_width() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic [7:0] in, ",
                "input logic [3:0] shamt, ",
                "output logic [7:0] left_shifted, ",
                "output logic [7:0] right_shifted, ",
                "output logic [7:0] right_past_width",
                "); ",
                "assign left_shifted = in << shamt; ",
                "assign right_shifted = in >> shamt; ",
                "assign right_past_width = in >> 4'd8; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("in".into(), 0x81), ("shamt".into(), 2)]))
        .expect("eval truncating shift case");
    assert_signal_eq!(outputs, "left_shifted", 0x04);
    assert_signal_eq!(outputs, "right_shifted", 0x20);
    assert_signal_eq!(outputs, "right_past_width", 0x00);

    let outputs = sim
        .eval_once(inputs([("in".into(), 0x03), ("shamt".into(), 6)]))
        .expect("eval large variable shift case");
    assert_signal_eq!(outputs, "left_shifted", 0xc0);
    assert_signal_eq!(outputs, "right_shifted", 0x00);
    assert_signal_eq!(outputs, "right_past_width", 0x00);
}

#[test]
fn eval_once_treats_unsized_decimal_literals_as_32_bit_values() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic [31:0] in, ",
                "output logic [31:0] out",
                "); ",
                "assign out = in & ~1; ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("in".into(), 21)]))
        .expect("eval masked value");
    assert_signal_eq!(outputs, "out", 20);
}

#[test]
fn eval_once_runs_part_select_rewrites() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/testing/013-Vector2.sv"))
        .expect("compile vector test");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([("in".into(), 0x1122_3344)]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 0x4433_2211);
}

#[test]
fn eval_once_runs_always_comb_case_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/mux_4to1_comb.sv"))
        .expect("compile mux_4to1_comb");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("d0".into(), 10),
            ("d1".into(), 20),
            ("d2".into(), 30),
            ("d3".into(), 40),
            ("sel".into(), 2),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 30);
}

#[test]
fn eval_once_runs_always_comb_if_else_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/alu_1bit.sv"))
        .expect("compile alu_1bit");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("a".into(), 0b1010_1010),
            ("b".into(), 0b1100_1100),
            ("op".into(), 0b01),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 0b1110_1110);
}

#[test]
fn eval_once_runs_always_comb_case_with_arithmetic() {
    let temp_dir = unique_temp_dir("always-comb-arithmetic");
    let source = r#"
module arithmetic_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic       sel,
    output logic [7:0] out
);
    always_comb
        if (sel == 1'b0)
            out = a + b;
        else
            out = a - b;
endmodule
"#;
    fs::write(temp_dir.join("arithmetic_ops.sv"), source).expect("write arithmetic_ops");

    let design = Compiler::new()
        .compile_file(temp_dir.join("arithmetic_ops.sv"))
        .expect("compile arithmetic_ops");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("a".into(), 5),
            ("b".into(), 3),
            ("sel".into(), 0),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 8);
}

#[test]
fn eval_once_runs_always_comb_with_logical_operators() {
    let temp_dir = unique_temp_dir("always-comb-logical");
    let source = r#"
module logical_ops (
    input  logic a,
    input  logic b,
    output logic out
);
    always_comb
        if ((a == 1'b0) && (b != 1'b0))
            out = 1'b1;
        else
            out = 1'b0;
endmodule
"#;
    fs::write(temp_dir.join("logical_ops.sv"), source).expect("write logical_ops");

    let design = Compiler::new()
        .compile_file(temp_dir.join("logical_ops.sv"))
        .expect("compile logical_ops");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([("a".into(), 0), ("b".into(), 1)]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 1);
}

#[test]
fn eval_once_runs_always_comb_with_relational_operators() {
    let temp_dir = unique_temp_dir("always-comb-relational");
    let source = r#"
module relational_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    output logic lt,
    output logic le,
    output logic gt,
    output logic ge
);
    assign lt = a < b;
    assign le = a <= b;
    assign gt = a > b;
    assign ge = a >= b;
endmodule
"#;
    fs::write(temp_dir.join("relational_ops.sv"), source).expect("write relational_ops");

    let design = Compiler::new()
        .compile_file(temp_dir.join("relational_ops.sv"))
        .expect("compile relational_ops");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("a".into(), 3), ("b".into(), 5)]))
        .expect("eval lt");
    assert_signal_eq!(outputs, "lt", 1);
    assert_signal_eq!(outputs, "le", 1);
    assert_signal_eq!(outputs, "gt", 0);
    assert_signal_eq!(outputs, "ge", 0);

    let outputs = sim
        .eval_once(inputs([("a".into(), 5), ("b".into(), 5)]))
        .expect("eval eq");
    assert_signal_eq!(outputs, "lt", 0);
    assert_signal_eq!(outputs, "le", 1);
    assert_signal_eq!(outputs, "gt", 0);
    assert_signal_eq!(outputs, "ge", 1);
}

#[test]
fn eval_once_supports_signed_cast_compare_and_shift() {
    let temp_dir = unique_temp_dir("signed-cast-ops");
    let source = r#"
module signed_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic [2:0] sh,
    output logic       lt,
    output logic [7:0] sra
);
    assign lt = $signed(a) < $signed(b);
    assign sra = $signed(a) >>> sh;
endmodule
"#;
    fs::write(temp_dir.join("signed_ops.sv"), source).expect("write signed_ops");

    let design = Compiler::new()
        .compile_file(temp_dir.join("signed_ops.sv"))
        .expect("compile signed_ops");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([
            ("a".into(), 0xf0),
            ("b".into(), 0x01),
            ("sh".into(), 2),
        ]))
        .expect("eval signed ops");

    assert_signal_eq!(outputs, "lt", 1);
    assert_signal_eq!(outputs, "sra", 0xfc);
}

#[test]
fn eval_once_supports_unsigned_cast_compare_and_unary_negation() {
    let temp_dir = unique_temp_dir("unsigned-cast-negate");
    let source = r#"
module unsigned_negate_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    output logic       lt_signed,
    output logic       lt_unsigned,
    output logic [7:0] neg
);
    assign lt_signed = $signed(a) < $signed(b);
    assign lt_unsigned = $unsigned($signed(a)) < $unsigned($signed(b));
    assign neg = -a;
endmodule
"#;
    fs::write(temp_dir.join("unsigned_negate_ops.sv"), source).expect("write unsigned_negate_ops");

    let design = Compiler::new()
        .compile_file(temp_dir.join("unsigned_negate_ops.sv"))
        .expect("compile unsigned_negate_ops");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("a".into(), 0xf0), ("b".into(), 0x01)]))
        .expect("eval unsigned negate ops");

    assert_signal_eq!(outputs, "lt_signed", 1);
    assert_signal_eq!(outputs, "lt_unsigned", 0);
    assert_signal_eq!(outputs, "neg", 0x10);
}

#[test]
fn eval_once_runs_always_comb_with_multiple_assignments_to_same_output() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/overture/overture_alu_8bit.sv"))
        .expect("compile overture_alu_8bit");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("inA".into(), 5),
            ("inB".into(), 3),
            ("op".into(), 0b100),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "outY", 8);
}

#[test]
fn step_runs_register_8bit_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/register_8bit.sv"))
        .expect("compile register_8bit");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = step_posedge(&mut sim, [("enable".into(), 1), ("data".into(), 0x5a)]);
    assert_signal_eq!(outputs, "q", 0x5a);

    let outputs = step_posedge(&mut sim, [("enable".into(), 0), ("data".into(), 0xff)]);
    assert_signal_eq!(outputs, "q", 0x5a);
}

#[test]
fn step_runs_counter_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/counter8.sv"))
        .expect("compile counter8");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = step_posedge(&mut sim, [("reset".into(), 1), ("enable".into(), 0)]);
    assert_signal_eq!(outputs, "count", 0);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("enable".into(), 1)]);
    assert_signal_eq!(outputs, "count", 1);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("enable".into(), 0)]);
    assert_signal_eq!(outputs, "count", 1);
}

#[test]
fn step_persists_blocking_assignments_in_clocked_blocks() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic clk, ",
                "input logic reset, ",
                "output logic [3:0] q",
                "); ",
                "always @(posedge clk) begin ",
                "if (reset) q = 4'd0; else q = q + 4'd1; ",
                "end ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = step_posedge(&mut sim, [("reset".into(), 1)]);
    assert_signal_eq!(outputs, "q", 0);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0)]);
    assert_signal_eq!(outputs, "q", 1);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0)]);
    assert_signal_eq!(outputs, "q", 2);
}

#[test]
fn step_runs_hierarchical_regfile() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/basic/regfile_8x8.sv"))
        .expect("compile regfile_8x8");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = step_posedge(
        &mut sim,
        [
            ("write_en".into(), 1),
            ("write_addr".into(), 3),
            ("write_data".into(), 0x42),
            ("read_addr1".into(), 3),
            ("read_addr2".into(), 0),
        ],
    );
    assert_signal_eq!(outputs, "read_data1", 0x42);

    let outputs = step_posedge(
        &mut sim,
        [
            ("write_en".into(), 1),
            ("write_addr".into(), 1),
            ("write_data".into(), 0x99),
            ("read_addr1".into(), 1),
            ("read_addr2".into(), 3),
        ],
    );
    assert_signal_eq!(outputs, "read_data1", 0x99);
    assert_signal_eq!(outputs, "read_data2", 0x42);
}

#[test]
fn step_runs_overture_pc_module() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/overture/overture_pc_8bit.sv"))
        .expect("compile overture_pc_8bit");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 1),
            ("run".into(), 0),
            ("jump_en".into(), 0),
            ("jump_addr".into(), 0),
        ],
    );
    assert_signal_eq!(outputs, "pc", 0);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("jump_en".into(), 0),
            ("jump_addr".into(), 0),
        ],
    );
    assert_signal_eq!(outputs, "pc", 1);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("jump_en".into(), 1),
            ("jump_addr".into(), 10),
        ],
    );
    assert_signal_eq!(outputs, "pc", 10);
}

#[test]
fn eval_once_reads_zero_initialized_memory() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/overture/overture_fetch.sv"))
        .expect("compile overture_fetch");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .eval_once(inputs([("addr".into(), 0x2a)]))
        .expect("eval");

    assert_signal_eq!(outputs, "data", 0);
}

#[test]
fn eval_once_reads_preloaded_memory() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/overture/overture_fetch.sv"))
        .expect("compile overture_fetch");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_words(&[], "rom", &words([0x12, 0x34, 0x56]))
        .expect("load rom");

    let outputs = sim.eval_once(inputs([("addr".into(), 1)])).expect("eval");

    assert_signal_eq!(outputs, "data", 0x34);
}

#[test]
fn eval_once_reads_memory_loaded_from_binary_text_file() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/overture/overture_fetch.sv"))
        .expect("compile overture_fetch");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_file(&[], "rom", repo.join("parts/basic/deadbeef.txt"))
        .expect("load rom from file");

    let outputs = sim.eval_once(inputs([("addr".into(), 2)])).expect("eval");

    assert_signal_eq!(outputs, "data", 0xbe);
}

#[test]
fn load_memory_file_supports_sparse_address_overrides() {
    let temp_dir = unique_temp_dir("memory-file-addresses");
    let memory_file = temp_dir.join("sparse_rom.txt");
    fs::write(
        &memory_file,
        "\
// leave address 0 untouched
2: 0x2a
3: 0b0000_1111
",
    )
    .expect("write sparse memory file");

    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/overture/overture_fetch.sv"))
        .expect("compile overture_fetch");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_file(&[], "rom", &memory_file)
        .expect("load rom from sparse file");

    let outputs = sim
        .eval_once(inputs([("addr".into(), 0)]))
        .expect("eval addr 0");
    assert_signal_eq!(outputs, "data", 0);

    let outputs = sim
        .eval_once(inputs([("addr".into(), 2)]))
        .expect("eval addr 2");
    assert_signal_eq!(outputs, "data", 0x2a);

    let outputs = sim
        .eval_once(inputs([("addr".into(), 3)]))
        .expect("eval addr 3");
    assert_signal_eq!(outputs, "data", 0x0f);
}

#[test]
fn step_runs_memory_cpu_stub_with_preloaded_rom_and_ram_write() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/testing/memory_cpu_stub.sv"))
        .expect("compile memory_cpu_stub");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_words(&[], "rom", &words([0x03, 0x42, 0x80, 0xc0]))
        .expect("load rom");

    let outputs = step_posedge(&mut sim, [("reset".into(), 1), ("run".into(), 0)]);
    assert_signal_eq!(outputs, "pc", 0);
    assert_signal_eq!(outputs, "acc", 0);
    assert_signal_eq!(outputs, "ram_out", 0);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("run".into(), 1)]);
    assert_signal_eq!(outputs, "pc", 1);
    assert_signal_eq!(outputs, "acc", 3);
    assert_signal_eq!(outputs, "ram_out", 0);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("run".into(), 1)]);
    assert_signal_eq!(outputs, "pc", 2);
    assert_signal_eq!(outputs, "acc", 5);
    assert_signal_eq!(outputs, "ram_out", 0);

    let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("run".into(), 1)]);
    assert_signal_eq!(outputs, "pc", 3);
    assert_signal_eq!(outputs, "acc", 5);
    assert_signal_eq!(outputs, "ram_out", 5);
    assert_eq!(
        sim.read_memory_word(&[], "ram", 0)
            .expect("read ram")
            .to_bit_value_checked(),
        Some(BitValue::from(5_u64))
    );
}

#[test]
fn step_runs_overture_cpu_with_preloaded_child_rom() {
    let repo = repo_root();
    let design = Compiler::new()
        .add_search_path(repo.join("parts/overture"))
        .compile_file(repo.join("parts/overture/overture_cpu.sv"))
        .expect("compile overture_cpu");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_words(&["fetch_unit"], "rom", &words([0x05]))
        .expect("load child rom");

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 1),
            ("run".into(), 0),
            ("in_port".into(), 0),
        ],
    );
    assert_signal_eq!(outputs, "pc", 0);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("in_port".into(), 0),
        ],
    );
    assert_signal_eq!(outputs, "pc", 1);
    assert_signal_eq!(outputs, "instr_debug", 0x05);
    assert_signal_eq!(outputs, "r0_out", 0x05);
    assert_eq!(
        sim.read_memory_word(&["fetch_unit"], "rom", 0)
            .expect("read child rom")
            .to_bit_value_checked(),
        Some(BitValue::from(0x05_u64))
    );
}

#[test]
fn read_signal_settles_overture_cpu_copy_path_from_input_port() {
    let repo = repo_root();
    let design = Compiler::new()
        .add_search_path(repo.join("parts/overture"))
        .compile_file(repo.join("parts/overture/overture_cpu.sv"))
        .expect("compile overture_cpu");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_file(
        &["fetch_unit"],
        "rom",
        repo.join("parts/overture/overture_add5.txt"),
    )
    .expect("load add5 program");

    let src_mux_module = design
        .hir()
        .module("mux_8to1_8bit")
        .expect("mux_8to1_8bit module");
    assert_eq!(
        src_mux_module
            .port("out")
            .expect("src_mux out port")
            .storage,
        crate::hir::StorageKind::Variable
    );

    let settled_inputs = inputs([("in_port".into(), 10), ("run".into(), 1)]);
    let settled_instr = sim
        .read_signal(&settled_inputs, &[], "instr")
        .expect("read settled instr");
    let settled_is_copy = sim
        .read_signal(&settled_inputs, &[], "is_copy")
        .expect("read settled is_copy");
    let settled_src_sel = sim
        .read_signal(&settled_inputs, &[], "src_sel")
        .expect("read settled src_sel");
    let settled_dst_sel = sim
        .read_signal(&settled_inputs, &[], "dst_sel")
        .expect("read settled dst_sel");
    let settled_mux_sel = sim
        .read_signal(&settled_inputs, &["src_mux"], "sel")
        .expect("read settled src_mux.sel");
    let settled_mux_in6 = sim
        .read_signal(&settled_inputs, &["src_mux"], "in6")
        .expect("read settled src_mux.in6");
    let settled_mux_hi = sim
        .read_signal(&settled_inputs, &["src_mux"], "mux_hi")
        .expect("read settled src_mux.mux_hi");
    let settled_final_sel = sim
        .read_signal(&settled_inputs, &["src_mux", "u_mux_out"], "sel")
        .expect("read settled src_mux.u_mux_out.sel");
    let settled_final_in1 = sim
        .read_signal(&settled_inputs, &["src_mux", "u_mux_out"], "in1")
        .expect("read settled src_mux.u_mux_out.in1");
    let settled_final_out = sim
        .read_signal(&settled_inputs, &["src_mux", "u_mux_out"], "out")
        .expect("read settled src_mux.u_mux_out.out");
    let settled_mux_out = sim
        .read_signal(&settled_inputs, &["src_mux"], "out")
        .expect("read settled src_mux.out");
    let settled_src_value = sim
        .read_signal(&settled_inputs, &[], "src_value")
        .expect("read settled src_value");

    assert_logic_eq!(settled_instr, 177);
    assert_logic_eq!(settled_is_copy, 1);
    assert_logic_eq!(settled_src_sel, 6);
    assert_logic_eq!(settled_dst_sel, 1);
    assert_logic_eq!(settled_mux_sel, 6);
    assert_logic_eq!(settled_mux_in6, 10);
    assert_logic_eq!(settled_mux_hi, 10);
    assert_logic_eq!(settled_final_sel, 1);
    assert_logic_eq!(settled_final_in1, 10);
    assert_logic_eq!(settled_final_out, 10);
    assert_logic_eq!(settled_mux_out, 10);
    assert_logic_eq!(settled_src_value, 10);
}

#[test]
fn step_runs_overture_cpu_add5_program() {
    let repo = repo_root();
    let design = Compiler::new()
        .add_search_path(repo.join("parts/overture"))
        .compile_file(repo.join("parts/overture/overture_cpu.sv"))
        .expect("compile overture_cpu");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_file(
        &["fetch_unit"],
        "rom",
        repo.join("parts/overture/overture_add5.txt"),
    )
    .expect("load add5 program");

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 1),
            ("run".into(), 0),
            ("in_port".into(), 0),
        ],
    );
    assert_signal_eq!(outputs, "pc", 0);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("in_port".into(), 10),
        ],
    );
    assert_signal_eq!(outputs, "pc", 1);
    assert_signal_eq!(outputs, "r1_out", 10);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("in_port".into(), 10),
        ],
    );
    assert_signal_eq!(outputs, "pc", 2);
    assert_signal_eq!(outputs, "r0_out", 5);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("in_port".into(), 10),
        ],
    );
    assert_signal_eq!(outputs, "pc", 3);
    assert_signal_eq!(outputs, "r2_out", 5);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("in_port".into(), 10),
        ],
    );
    assert_signal_eq!(outputs, "pc", 4);
    assert_signal_eq!(outputs, "r3_out", 15);

    let outputs = step_posedge(
        &mut sim,
        [
            ("reset".into(), 0),
            ("run".into(), 1),
            ("in_port".into(), 10),
        ],
    );
    assert_signal_eq!(outputs, "pc", 5);
    assert_signal_eq!(outputs, "out_port", 15);
}

#[test]
fn instantiate_top_supports_picorv32_parameterized_wrapper() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/picorv32/picorv32.v"))
        .expect("compile picorv32");

    let _sim = design.instantiate_top().expect("instantiate picorv32_wb");
}

#[test]
fn eval_once_applies_named_parameter_overrides_from_parent_modules() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module leaf #(parameter [7:0] VALUE = 8'h11)(",
                "output logic [7:0] out",
                "); ",
                "assign out = VALUE; ",
                "endmodule\n",
                "module top #(parameter [7:0] VALUE = 8'h2a)(",
                "output logic [7:0] out",
                "); ",
                "leaf #(.VALUE(VALUE)) u_leaf(.out(out)); ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim.eval_once(BTreeMap::new()).expect("eval");

    assert_signal_eq!(outputs, "out", 0x2a);
}

#[test]
fn instantiate_rejects_override_of_range_frozen_parameter() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module leaf #(parameter WIDTH = 8)(",
                "input [WIDTH-1:0] a, output [WIDTH-1:0] y",
                "); ",
                "assign y = ~a; ",
                "endmodule\n",
                "module top(input [7:0] a, output [7:0] y); ",
                "leaf #(.WIDTH(4)) u_leaf(.a(a), .y(y)); ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");

    let error = design
        .instantiate_top()
        .expect_err("frozen-range override must be rejected")
        .to_string();
    assert!(
        error.contains("parameter 'WIDTH' of module 'leaf' is frozen into")
            && error.contains("a packed declaration range")
            && error.contains("u_leaf"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn instantiate_allows_override_equal_to_frozen_default() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module leaf #(parameter WIDTH = 8)(",
                "input [WIDTH-1:0] a, output [WIDTH-1:0] y",
                "); ",
                "assign y = ~a; ",
                "endmodule\n",
                "module top(input [7:0] a, output [7:0] y); ",
                "leaf #(.WIDTH(8)) u_leaf(.a(a), .y(y)); ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(BTreeMap::from([(
            "a".to_string(),
            LogicValue::from(BitValue::from(0x0f_u64)),
        )]))
        .expect("eval");

    assert_signal_eq!(outputs, "y", 0xf0);
}

#[test]
fn instantiate_rejects_override_that_shifts_dependent_frozen_localparam() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module leaf #(parameter BASE = 2)(",
                "output [7:0] y",
                "); ",
                "localparam W = BASE * 2; ",
                "wire [W-1:0] inner = {W{1'b1}}; ",
                "assign y = {4'b0000, inner}; ",
                "endmodule\n",
                "module top(output [7:0] y); ",
                "leaf #(.BASE(3)) u_leaf(.y(y)); ",
                "endmodule\n"
            ),
        )
        .expect("compile virtual design");

    let error = design
        .instantiate_top()
        .expect_err("override shifting a frozen dependent localparam must be rejected")
        .to_string();
    assert!(
        error.contains("parameter 'W' of module 'leaf' is frozen into"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn step_runs_picorv32_smoke_store_sequence() {
    let repo = repo_root();
    let design = Compiler::new()
        .add_search_path(repo.join("parts/picorv32"))
        .compile_file(repo.join("parts/picorv32/picorv32_smoke.sv"))
        .expect("compile picorv32 smoke harness");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_file(
        &[],
        "rom",
        repo.join("parts/picorv32/picorv32_smoke_rom.txt"),
    )
    .expect("load smoke rom");

    step_posedge(&mut sim, [("resetn".into(), 0)]);

    for _ in 0..9 {
        step_posedge(&mut sim, [("resetn".into(), 1)]);
    }

    let core = sim
        .state
        .children
        .first()
        .expect("child instance")
        .state
        .as_ref();
    assert_eq!(memory_u64(core, "cpuregs", 1), 1);
    assert_eq!(persisted_u64(&sim, core, "mem_do_wdata"), 1);
    assert_eq!(persisted_u64(&sim, core, "reg_op1"), 8);
    assert_eq!(persisted_u64(&sim, core, "reg_op2"), 1);

    let outputs = step_posedge(&mut sim, [("resetn".into(), 1)]);
    assert_signal_eq!(outputs, "trap", 0);
    assert_signal_eq!(outputs, "mem_valid", 1);
    assert_signal_eq!(outputs, "mem_instr", 0);
    assert_signal_eq!(outputs, "mem_addr", 8);
    assert_signal_eq!(outputs, "store_seen", 0);

    let outputs = step_posedge(&mut sim, [("resetn".into(), 1)]);
    assert_signal_eq!(outputs, "store_seen", 1);
    assert_signal_eq!(outputs, "store_addr", 8);
    assert_signal_eq!(outputs, "store_data", 1);
}

#[test]
fn step_runs_always_ff_only_on_rising_edges() {
    let temp_dir = unique_temp_dir("always-ff-rising-edge");
    let design = Compiler::new()
        .compile_str(
            temp_dir.join("edge_counter.sv"),
            r#"
module edge_counter(
    input  logic clk,
    output logic [7:0] count
);
    always_ff @(posedge clk)
        count <= count + 1'b1;
endmodule
"#,
        )
        .expect("compile edge_counter");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim.step(inputs([("clk".into(), 0)])).expect("step low");
    assert_signal_eq!(outputs, "count", 0);

    let outputs = sim.step(inputs([("clk".into(), 1)])).expect("step rise");
    assert_signal_eq!(outputs, "count", 1);

    let outputs = sim.step(inputs([("clk".into(), 1)])).expect("step high");
    assert_signal_eq!(outputs, "count", 1);

    let outputs = sim.step(inputs([("clk".into(), 0)])).expect("step fall");
    assert_signal_eq!(outputs, "count", 1);

    let outputs = sim
        .step(inputs([("clk".into(), 1)]))
        .expect("step rise again");
    assert_signal_eq!(outputs, "count", 2);
}

#[test]
fn step_runs_always_ff_on_async_reset_edges() {
    let temp_dir = unique_temp_dir("always-ff-async-reset");
    let design = Compiler::new()
        .compile_str(
            temp_dir.join("async_reset_counter.sv"),
            r#"
module async_reset_counter(
    input  logic clk,
    input  logic reset,
    output logic [7:0] count
);
    always_ff @(posedge clk or posedge reset)
        if (reset)
            count <= 8'd0;
        else
            count <= count + 1'b1;
endmodule
"#,
        )
        .expect("compile async_reset_counter");
    let mut sim = design.instantiate_top().expect("instantiate");

    let outputs = sim
        .step(inputs([("clk".into(), 1), ("reset".into(), 0)]))
        .expect("step rise");
    assert_signal_eq!(outputs, "count", 1);

    let outputs = sim
        .step(inputs([("clk".into(), 0), ("reset".into(), 1)]))
        .expect("step async reset rise");
    assert_signal_eq!(outputs, "count", 0);

    let outputs = sim
        .step(inputs([("clk".into(), 1), ("reset".into(), 1)]))
        .expect("step with reset held");
    assert_signal_eq!(outputs, "count", 0);

    let outputs = sim
        .step(inputs([("clk".into(), 0), ("reset".into(), 0)]))
        .expect("release reset");
    assert_signal_eq!(outputs, "count", 0);

    let outputs = sim
        .step(inputs([("clk".into(), 1), ("reset".into(), 0)]))
        .expect("step post-reset rise");
    assert_signal_eq!(outputs, "count", 1);
}

#[test]
fn load_memory_file_reads_decimal_program_file_into_child_rom() {
    let repo = repo_root();
    let design = Compiler::new()
        .add_search_path(repo.join("parts/overture"))
        .compile_file(repo.join("parts/overture/overture_cpu.sv"))
        .expect("compile overture_cpu");
    let mut sim = design.instantiate_top().expect("instantiate");
    sim.load_memory_file(
        &["fetch_unit"],
        "rom",
        repo.join("parts/overture/overture_prog_alu.txt"),
    )
    .expect("load overture program");

    assert_eq!(
        sim.read_memory_word(&["fetch_unit"], "rom", 0)
            .expect("read instruction 0")
            .to_bit_value_checked(),
        Some(BitValue::from(0x05_u64))
    );
    assert_eq!(
        sim.read_memory_word(&["fetch_unit"], "rom", 1)
            .expect("read instruction 1")
            .to_bit_value_checked(),
        Some(BitValue::from(0x81_u64))
    );
    assert_eq!(
        sim.read_memory_word(&["fetch_unit"], "rom", 16)
            .expect("read instruction 16")
            .to_bit_value_checked(),
        Some(BitValue::from(0x9e_u64))
    );
}

#[test]
fn eval_once_runs_vector_concatenation_assignment() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/testing/016-Vector3.sv"))
        .expect("compile 016-Vector3");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("a".into(), 31),
            ("b".into(), 21),
            ("c".into(), 10),
            ("d".into(), 5),
            ("e".into(), 3),
            ("f".into(), 1),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "w", 253);
    assert_signal_eq!(outputs, "x", 84);
    assert_signal_eq!(outputs, "y", 81);
    assert_signal_eq!(outputs, "z", 135);
}

#[test]
fn eval_once_runs_bit_reversal_concatenation() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/testing/017-Vectorr.sv"))
        .expect("compile 017-Vectorr");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([("in".into(), 0b1101_0011)]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 0b1100_1011);
}

#[test]
fn read_signal_reads_settled_child_signal_values() {
    let temp_dir = unique_temp_dir("read-hier-signal");
    let design = Compiler::new()
        .compile_str(
            temp_dir.join("top.sv"),
            concat!(
                "module leaf(",
                "input logic [7:0] a, ",
                "output logic [7:0] out",
                "); ",
                "logic [7:0] mirrored; ",
                "assign mirrored = a + 8'd1; ",
                "assign out = mirrored; ",
                "endmodule\n",
                "module top(",
                "input logic [7:0] in, ",
                "output logic [7:0] out",
                "); ",
                "leaf u_leaf(.a(in), .out(out)); ",
                "endmodule\n"
            ),
        )
        .expect("compile top");
    let mut sim = design.instantiate_top().expect("instantiate");
    let in_inputs = inputs([("in".into(), 5)]);
    let outputs = sim.eval_once(in_inputs.clone()).expect("eval");

    assert_signal_eq!(outputs, "out", 6);
    assert_logic_eq!(
        sim.read_signal(&in_inputs, &["u_leaf"], "mirrored")
            .expect("read child signal"),
        6
    );
}

#[test]
fn eval_once_runs_sign_extension_replication() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/testing/018-Vector4SignExtension.sv"))
        .expect("compile 018-Vector4SignExtension");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim.eval_once(inputs([("in".into(), 0x81)])).expect("eval");

    assert_signal_eq!(outputs, "out", 0xffff_ff81);
}

#[test]
fn eval_once_runs_multi_expression_replication_with_sv_bit_order() {
    let repo = repo_root();
    let design = Compiler::new()
        .compile_file(repo.join("parts/testing/019-Vector5.sv"))
        .expect("compile 019-Vector5");
    let mut sim = design.instantiate_top().expect("instantiate");
    let outputs = sim
        .eval_once(inputs([
            ("a".into(), 1),
            ("b".into(), 0),
            ("c".into(), 1),
            ("d".into(), 0),
            ("e".into(), 1),
        ]))
        .expect("eval");

    assert_signal_eq!(outputs, "out", 22_369_621);
}

#[test]
fn eval_once_runs_arbitrary_width_passthrough() {
    let design = Compiler::new()
        .compile_str(
            PathBuf::from("/virtual/top.sv"),
            concat!(
                "module top(",
                "input logic [191:0] inA, ",
                "output logic [191:0] outY",
                "); ",
                "assign outY = inA; ",
                "endmodule\n"
            ),
        )
        .expect("compile wide passthrough");
    let mut sim = design.instantiate_top().expect("instantiate");
    let input = BitValue::from_prefixed_str("0x1234567890abcdef1234567890abcdef1234567890abcdef")
        .expect("parse wide input");
    let outputs = sim
        .eval_once(BTreeMap::from([(
            "inA".into(),
            LogicValue::from(input.clone()),
        )]))
        .expect("eval");

    assert_logic_bits_eq!(outputs.get("outY").expect("wide output").clone(), input);
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    path.push(format!("svsim-{name}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}
