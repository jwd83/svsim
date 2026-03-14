use std::collections::{BTreeMap, HashMap};

use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::hir::{
    AssignmentKind, BinaryOp, Expr, HirDesign, LValue, ModuleInstanceSummary, ModuleSummary,
    NumericLiteral, PortDirection, ProcBlockKind, Stmt, UnaryOp,
};

#[derive(Debug, Clone)]
pub struct SimulationSession {
    design: CompiledDesign,
    state: ModuleState,
}

#[derive(Debug, Clone)]
struct ModuleState {
    module_name: String,
    persisted: HashMap<String, Value>,
    children: Vec<ChildState>,
}

#[derive(Debug, Clone)]
struct ChildState {
    state: Box<ModuleState>,
}

impl SimulationSession {
    pub(crate) fn new(design: CompiledDesign) -> Result<Self> {
        let top_module = design
            .top_module()
            .expect("compiled designs always carry a top module");
        let mut stack = Vec::new();
        let state = instantiate_module_state(design.hir(), top_module, &mut stack)?;
        Ok(Self { design, state })
    }

    pub fn top_module(&self) -> &str {
        self.design
            .top_module()
            .expect("compiled designs always carry a top module")
    }

    pub fn eval_once(&mut self, inputs: BTreeMap<String, u64>) -> Result<BTreeMap<String, u64>> {
        let module = top_module(self.design.hir(), self.top_module())?;
        let mut stack = Vec::new();
        let values = settle_module(self.design.hir(), module, &self.state, &inputs, &mut stack)?;
        Ok(collect_outputs(module, &values))
    }

    pub fn step(&mut self, inputs: BTreeMap<String, u64>) -> Result<BTreeMap<String, u64>> {
        let module = top_module(self.design.hir(), self.top_module())?;
        let mut stack = Vec::new();
        step_module(self.design.hir(), &mut self.state, &inputs, &mut stack)?;

        let mut settle_stack = Vec::new();
        let values = settle_module(
            self.design.hir(),
            module,
            &self.state,
            &inputs,
            &mut settle_stack,
        )?;
        Ok(collect_outputs(module, &values))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Value {
    bits: u64,
    width: usize,
}

impl Value {
    fn new(bits: u64, width: usize) -> Self {
        let width = width.max(1);
        Self {
            bits: bits & mask(width),
            width,
        }
    }

    fn zero(width: usize) -> Self {
        Self::new(0, width)
    }

    fn normalized_bits(self) -> u64 {
        self.bits & mask(self.width)
    }

    fn truthy(self) -> bool {
        self.normalized_bits() != 0
    }
}

fn top_module<'a>(hir: &'a HirDesign, module_name: &str) -> Result<&'a ModuleSummary> {
    resolve_supported_module(hir, module_name)
}

fn instantiate_module_state(
    hir: &HirDesign,
    module_name: &str,
    stack: &mut Vec<String>,
) -> Result<ModuleState> {
    if stack.iter().any(|name| name == module_name) {
        return Err(Error::Unsupported(format!(
            "recursive instantiation detected at {} -> {}",
            stack.join(" -> "),
            module_name
        )));
    }

    let module = resolve_supported_module(hir, module_name)?;
    stack.push(module_name.to_owned());

    let mut children = Vec::with_capacity(module.instantiations.len());
    for instance in &module.instantiations {
        children.push(ChildState {
            state: Box::new(instantiate_module_state(hir, &instance.module_name, stack)?),
        });
    }

    stack.pop();
    Ok(ModuleState {
        module_name: module_name.to_owned(),
        persisted: build_persisted_signal_table(module),
        children,
    })
}

fn settle_module(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &ModuleState,
    inputs: &BTreeMap<String, u64>,
    stack: &mut Vec<String>,
) -> Result<HashMap<String, Value>> {
    if stack.iter().any(|name| name == &state.module_name) {
        return Err(Error::Unsupported(format!(
            "recursive combinational instantiation detected at {} -> {}",
            stack.join(" -> "),
            state.module_name
        )));
    }

    let mut values = build_signal_table(module, inputs, &state.persisted)?;
    let max_iterations = ((module.continuous_assignments.len()
        + module.proc_blocks.len()
        + module.instantiations.len()
        + values.len())
    .max(1))
        * 8;

    stack.push(state.module_name.clone());
    let mut converged = false;
    for _ in 0..max_iterations {
        let mut changed = false;

        for assign in &module.continuous_assignments {
            let value = eval_expr(&assign.expr, &values)?;
            changed |= apply_lvalue(&assign.target, value, module, &mut values)?;
        }

        for block in &module.proc_blocks {
            changed |= execute_proc_block(&block.kind, &block.body, module, &mut values)?;
        }

        for (instance, child_state) in module.instantiations.iter().zip(&state.children) {
            changed |= evaluate_instance(
                hir,
                module,
                instance,
                child_state.state.as_ref(),
                &mut values,
                stack,
            )?;
        }

        if !changed {
            converged = true;
            break;
        }
    }
    stack.pop();

    if !converged {
        return Err(Error::Unsupported(format!(
            "combinational evaluation did not converge for module '{}'",
            module.name
        )));
    }

    Ok(values)
}

fn step_module(
    hir: &HirDesign,
    state: &mut ModuleState,
    inputs: &BTreeMap<String, u64>,
    stack: &mut Vec<String>,
) -> Result<()> {
    let module = resolve_supported_module(hir, &state.module_name)?;
    let pre_values = settle_module(hir, module, state, inputs, stack)?;

    for (instance, child_state) in module.instantiations.iter().zip(state.children.iter_mut()) {
        let child = resolve_supported_module(hir, &instance.module_name)?;
        let child_inputs = build_child_inputs(child, instance, &pre_values)?;
        step_module(hir, child_state.state.as_mut(), &child_inputs, stack)?;
    }

    let mut staged = state.persisted.clone();
    for block in &module.proc_blocks {
        match &block.kind {
            ProcBlockKind::AlwaysComb => {}
            ProcBlockKind::AlwaysFf { clock } => {
                let clock_value = pre_values.get(clock).copied().ok_or_else(|| {
                    Error::Resolve(format!(
                        "clock '{}' is not declared in '{}'",
                        clock, module.name
                    ))
                })?;
                if clock_value.truthy() {
                    execute_sequential_stmt(&block.body, module, &pre_values, &mut staged)?;
                }
            }
        }
    }

    state.persisted = staged;
    Ok(())
}

fn execute_proc_block(
    kind: &ProcBlockKind,
    body: &Stmt,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
) -> Result<bool> {
    match kind {
        ProcBlockKind::AlwaysComb => execute_comb_stmt(body, module, values),
        ProcBlockKind::AlwaysFf { .. } => Ok(false),
    }
}

fn execute_comb_stmt(
    stmt: &Stmt,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
) -> Result<bool> {
    match stmt {
        Stmt::Empty => Ok(false),
        Stmt::Block(statements) => {
            let mut changed = false;
            for statement in statements {
                changed |= execute_comb_stmt(statement, module, values)?;
            }
            Ok(changed)
        }
        Stmt::Assign { kind, target, expr } => match kind {
            AssignmentKind::Blocking => {
                let value = eval_expr(expr, values)?;
                apply_lvalue(target, value, module, values)
            }
            AssignmentKind::Nonblocking => Err(Error::Unsupported(
                "nonblocking assignments are only supported inside `always_ff` blocks".into(),
            )),
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if eval_expr(cond, values)?.truthy() {
                execute_comb_stmt(then_branch, module, values)
            } else if let Some(else_branch) = else_branch {
                execute_comb_stmt(else_branch, module, values)
            } else {
                Ok(false)
            }
        }
        Stmt::Case {
            expr,
            items,
            default,
        } => {
            let value = eval_expr(expr, values)?;
            for item in items {
                for pattern in &item.patterns {
                    if values_equal(value, eval_expr(pattern, values)?) {
                        return execute_comb_stmt(&item.body, module, values);
                    }
                }
            }
            if let Some(default) = default {
                execute_comb_stmt(default, module, values)
            } else {
                Ok(false)
            }
        }
    }
}

fn execute_sequential_stmt(
    stmt: &Stmt,
    module: &ModuleSummary,
    current_values: &HashMap<String, Value>,
    staged_values: &mut HashMap<String, Value>,
) -> Result<()> {
    match stmt {
        Stmt::Empty => Ok(()),
        Stmt::Block(statements) => {
            for statement in statements {
                execute_sequential_stmt(statement, module, current_values, staged_values)?;
            }
            Ok(())
        }
        Stmt::Assign { kind, target, expr } => match kind {
            AssignmentKind::Nonblocking => {
                let value = eval_expr(expr, current_values)?;
                apply_lvalue(target, value, module, staged_values)?;
                Ok(())
            }
            AssignmentKind::Blocking => Err(Error::Unsupported(
                "blocking assignments inside `always_ff` blocks are not supported yet".into(),
            )),
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if eval_expr(cond, current_values)?.truthy() {
                execute_sequential_stmt(then_branch, module, current_values, staged_values)
            } else if let Some(else_branch) = else_branch {
                execute_sequential_stmt(else_branch, module, current_values, staged_values)
            } else {
                Ok(())
            }
        }
        Stmt::Case {
            expr,
            items,
            default,
        } => {
            let value = eval_expr(expr, current_values)?;
            for item in items {
                for pattern in &item.patterns {
                    if values_equal(value, eval_expr(pattern, current_values)?) {
                        return execute_sequential_stmt(
                            &item.body,
                            module,
                            current_values,
                            staged_values,
                        );
                    }
                }
            }
            if let Some(default) = default {
                execute_sequential_stmt(default, module, current_values, staged_values)
            } else {
                Ok(())
            }
        }
    }
}

fn build_persisted_signal_table(module: &ModuleSummary) -> HashMap<String, Value> {
    let mut values = HashMap::new();

    for port in &module.ports {
        values.insert(port.name.clone(), Value::zero(port.width()));
    }
    for signal in &module.signals {
        values.insert(signal.name.clone(), Value::zero(signal.width()));
    }

    values
}

fn build_signal_table(
    module: &ModuleSummary,
    inputs: &BTreeMap<String, u64>,
    persisted: &HashMap<String, Value>,
) -> Result<HashMap<String, Value>> {
    let mut values = HashMap::new();

    for port in &module.ports {
        let value = if matches!(port.direction, PortDirection::Input) {
            Value::new(*inputs.get(&port.name).unwrap_or(&0), port.width())
        } else {
            persisted
                .get(&port.name)
                .copied()
                .unwrap_or_else(|| Value::zero(port.width()))
        };
        values.insert(port.name.clone(), value);
    }

    for signal in &module.signals {
        values.insert(
            signal.name.clone(),
            persisted
                .get(&signal.name)
                .copied()
                .unwrap_or_else(|| Value::zero(signal.width())),
        );
    }

    for name in inputs.keys() {
        if module.port(name).is_none() {
            return Err(Error::Resolve(format!(
                "input '{}' does not match any port on module '{}'",
                name, module.name
            )));
        }
    }

    Ok(values)
}

fn build_child_inputs(
    child: &ModuleSummary,
    instance: &ModuleInstanceSummary,
    parent_values: &HashMap<String, Value>,
) -> Result<BTreeMap<String, u64>> {
    let mut child_inputs = BTreeMap::new();

    for port in child
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input))
    {
        let Some(connection) = find_connection(instance, &port.name) else {
            continue;
        };
        let value = eval_expr(&connection.expr, parent_values)?;
        child_inputs.insert(port.name.clone(), value.normalized_bits());
    }

    Ok(child_inputs)
}

fn evaluate_instance(
    hir: &HirDesign,
    parent: &ModuleSummary,
    instance: &ModuleInstanceSummary,
    child_state: &ModuleState,
    values: &mut HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Result<bool> {
    let child = resolve_supported_module(hir, &instance.module_name).map_err(|_| {
        Error::Resolve(format!(
            "instance '{}' references missing module '{}'",
            instance.instance_name, instance.module_name
        ))
    })?;

    let child_inputs = build_child_inputs(child, instance, values)?;
    let child_values = settle_module(hir, child, child_state, &child_inputs, stack)?;
    let mut changed = false;

    for port in child
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output))
    {
        let Some(connection) = find_connection(instance, &port.name) else {
            continue;
        };
        let lvalue = expr_to_lvalue(&connection.expr).ok_or_else(|| {
            Error::Unsupported(format!(
                "instance '{}' connects output port '{}' to a non-lvalue expression",
                instance.instance_name, port.name
            ))
        })?;
        let value = child_values
            .get(&port.name)
            .copied()
            .unwrap_or_else(|| Value::zero(port.width()));
        changed |= apply_lvalue(&lvalue, value, parent, values)?;
    }

    Ok(changed)
}

fn collect_outputs(
    module: &ModuleSummary,
    values: &HashMap<String, Value>,
) -> BTreeMap<String, u64> {
    let mut outputs = BTreeMap::new();

    for port in module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output))
    {
        let value = values
            .get(&port.name)
            .copied()
            .unwrap_or_else(|| Value::zero(port.width()));
        outputs.insert(port.name.clone(), value.normalized_bits());
    }

    outputs
}

fn resolve_supported_module<'a>(
    hir: &'a HirDesign,
    module_name: &str,
) -> Result<&'a ModuleSummary> {
    let module = hir
        .module(module_name)
        .ok_or_else(|| Error::Resolve(format!("module '{}' was not compiled", module_name)))?;
    if !module.unsupported.is_empty() {
        return Err(Error::Unsupported(format!(
            "module '{}' uses unsupported constructs: {}",
            module_name,
            module
                .unsupported
                .iter()
                .map(|diag| diag.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    Ok(module)
}

fn find_connection<'a>(
    instance: &'a ModuleInstanceSummary,
    port_name: &str,
) -> Option<&'a crate::hir::NamedPortConnection> {
    instance
        .connections
        .iter()
        .find(|connection| connection.port_name == port_name)
}

fn eval_expr(expr: &Expr, values: &HashMap<String, Value>) -> Result<Value> {
    match expr {
        Expr::Ident(name) => values
            .get(name)
            .copied()
            .ok_or_else(|| Error::Resolve(format!("signal '{}' is not declared", name))),
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        Expr::BitSelect { expr, index } => {
            let value = eval_expr(expr, values)?;
            if *index >= value.width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for width {}",
                    index, value.width
                )));
            }
            Ok(Value::new((value.normalized_bits() >> index) & 1, 1))
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let value = eval_expr(expr, values)?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            if high >= value.width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for width {}",
                    msb, lsb, value.width
                )));
            }
            let width = high - low + 1;
            Ok(Value::new(
                (value.normalized_bits() >> low) & mask(width),
                width,
            ))
        }
        Expr::Unary { op, expr } => {
            let value = eval_expr(expr, values)?;
            match op {
                UnaryOp::BitNot => Ok(Value::new(!value.normalized_bits(), value.width)),
            }
        }
        Expr::Binary { left, op, right } => {
            let left = eval_expr(left, values)?;
            let right = eval_expr(right, values)?;
            let (bits, width) = match op {
                BinaryOp::BitAnd => (
                    left.normalized_bits() & right.normalized_bits(),
                    left.width.max(right.width),
                ),
                BinaryOp::BitOr => (
                    left.normalized_bits() | right.normalized_bits(),
                    left.width.max(right.width),
                ),
                BinaryOp::BitXor => (
                    left.normalized_bits() ^ right.normalized_bits(),
                    left.width.max(right.width),
                ),
                BinaryOp::LogicalAnd => ((left.truthy() && right.truthy()) as u64, 1),
                BinaryOp::LogicalOr => ((left.truthy() || right.truthy()) as u64, 1),
                BinaryOp::Eq => (values_equal(left, right) as u64, 1),
                BinaryOp::NotEq => ((!values_equal(left, right)) as u64, 1),
                BinaryOp::Add => (
                    left.normalized_bits().wrapping_add(right.normalized_bits()),
                    left.width.max(right.width),
                ),
                BinaryOp::Sub => (
                    left.normalized_bits().wrapping_sub(right.normalized_bits()),
                    left.width.max(right.width),
                ),
            };
            Ok(Value::new(bits, width))
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            if eval_expr(cond, values)?.truthy() {
                eval_expr(when_true, values)
            } else {
                eval_expr(when_false, values)
            }
        }
    }
}

fn value_from_literal(literal: &NumericLiteral) -> Value {
    let width = literal.width.unwrap_or_else(|| minimum_width(literal.bits));
    Value::new(literal.bits, width)
}

fn values_equal(left: Value, right: Value) -> bool {
    left.normalized_bits() == right.normalized_bits()
}

fn expr_to_lvalue(expr: &Expr) -> Option<LValue> {
    match expr {
        Expr::Ident(name) => Some(LValue::Signal(name.clone())),
        Expr::BitSelect { expr, index } => match expr.as_ref() {
            Expr::Ident(name) => Some(LValue::BitSelect {
                signal: name.clone(),
                index: *index,
            }),
            _ => None,
        },
        Expr::PartSelect { expr, msb, lsb } => match expr.as_ref() {
            Expr::Ident(name) => Some(LValue::PartSelect {
                signal: name.clone(),
                msb: *msb,
                lsb: *lsb,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn apply_lvalue(
    lvalue: &LValue,
    value: Value,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
) -> Result<bool> {
    match lvalue {
        LValue::Signal(name) => {
            let current = values.get_mut(name).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                ))
            })?;
            let next = Value::new(value.normalized_bits(), current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        LValue::BitSelect { signal, index } => {
            let current = values.get_mut(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            if *index >= current.width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for signal '{}'",
                    index, signal
                )));
            }
            let bit = value.normalized_bits() & 1;
            let mut bits = current.normalized_bits();
            bits &= !(1u64 << index);
            bits |= bit << index;
            let next = Value::new(bits, current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        LValue::PartSelect { signal, msb, lsb } => {
            let current = values.get_mut(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            if high >= current.width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for signal '{}'",
                    msb, lsb, signal
                )));
            }
            let width = high - low + 1;
            let select_mask = mask(width) << low;
            let mut bits = current.normalized_bits();
            bits &= !select_mask;
            bits |= (value.normalized_bits() & mask(width)) << low;
            let next = Value::new(bits, current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
    }
}

fn minimum_width(bits: u64) -> usize {
    if bits == 0 {
        1
    } else {
        (u64::BITS - bits.leading_zeros()) as usize
    }
}

fn mask(width: usize) -> u64 {
    if width >= u64::BITS as usize {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::Compiler;

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
            .eval_once(BTreeMap::from([("inA".into(), 1), ("inB".into(), 1)]))
            .expect("eval");

        assert_eq!(outputs.get("outY"), Some(&0));
    }

    #[test]
    fn eval_once_runs_hierarchical_combinational_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/full_adder.sv"))
            .expect("compile full adder");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(BTreeMap::from([
                ("inA".into(), 1),
                ("inB".into(), 1),
                ("inCarry".into(), 1),
            ]))
            .expect("eval");

        assert_eq!(outputs.get("outSum"), Some(&1));
        assert_eq!(outputs.get("outCarry"), Some(&1));
    }

    #[test]
    fn eval_once_runs_vector_ternary_assign() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/ternary_mux.sv"))
            .expect("compile ternary mux");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(BTreeMap::from([
                ("a".into(), 0x12),
                ("b".into(), 0x34),
                ("sel".into(), 1),
            ]))
            .expect("eval");

        assert_eq!(outputs.get("out"), Some(&0x12));
    }

    #[test]
    fn eval_once_runs_part_select_rewrites() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/013-Vector2.sv"))
            .expect("compile vector test");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(BTreeMap::from([("in".into(), 0x1122_3344)]))
            .expect("eval");

        assert_eq!(outputs.get("out"), Some(&0x4433_2211));
    }

    #[test]
    fn eval_once_runs_always_comb_case_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/mux_4to1_comb.sv"))
            .expect("compile mux_4to1_comb");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(BTreeMap::from([
                ("d0".into(), 10),
                ("d1".into(), 20),
                ("d2".into(), 30),
                ("d3".into(), 40),
                ("sel".into(), 2),
            ]))
            .expect("eval");

        assert_eq!(outputs.get("out"), Some(&30));
    }

    #[test]
    fn eval_once_runs_always_comb_if_else_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/alu_1bit.sv"))
            .expect("compile alu_1bit");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(BTreeMap::from([
                ("a".into(), 0b1010_1010),
                ("b".into(), 0b1100_1100),
                ("op".into(), 0b01),
            ]))
            .expect("eval");

        assert_eq!(outputs.get("out"), Some(&0b1110_1110));
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
            .eval_once(BTreeMap::from([
                ("a".into(), 5),
                ("b".into(), 3),
                ("sel".into(), 0),
            ]))
            .expect("eval");

        assert_eq!(outputs.get("out"), Some(&8));
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
            .eval_once(BTreeMap::from([("a".into(), 0), ("b".into(), 1)]))
            .expect("eval");

        assert_eq!(outputs.get("out"), Some(&1));
    }

    #[test]
    fn step_runs_register_8bit_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/register_8bit.sv"))
            .expect("compile register_8bit");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("enable".into(), 1),
                ("data".into(), 0x5a),
            ]))
            .expect("step");
        assert_eq!(outputs.get("q"), Some(&0x5a));

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("enable".into(), 0),
                ("data".into(), 0xff),
            ]))
            .expect("hold");
        assert_eq!(outputs.get("q"), Some(&0x5a));
    }

    #[test]
    fn step_runs_counter_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/counter8.sv"))
            .expect("compile counter8");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("reset".into(), 1),
                ("enable".into(), 0),
            ]))
            .expect("reset");
        assert_eq!(outputs.get("count"), Some(&0));

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("enable".into(), 1),
            ]))
            .expect("increment");
        assert_eq!(outputs.get("count"), Some(&1));

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("enable".into(), 0),
            ]))
            .expect("hold");
        assert_eq!(outputs.get("count"), Some(&1));
    }

    #[test]
    fn step_runs_hierarchical_regfile() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/regfile_8x8.sv"))
            .expect("compile regfile_8x8");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("write_en".into(), 1),
                ("write_addr".into(), 3),
                ("write_data".into(), 0x42),
                ("read_addr1".into(), 3),
                ("read_addr2".into(), 0),
            ]))
            .expect("write r3");
        assert_eq!(outputs.get("read_data1"), Some(&0x42));

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("write_en".into(), 1),
                ("write_addr".into(), 1),
                ("write_data".into(), 0x99),
                ("read_addr1".into(), 1),
                ("read_addr2".into(), 3),
            ]))
            .expect("write r1");
        assert_eq!(outputs.get("read_data1"), Some(&0x99));
        assert_eq!(outputs.get("read_data2"), Some(&0x42));
    }

    #[test]
    fn step_runs_overture_pc_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_pc_8bit.sv"))
            .expect("compile overture_pc_8bit");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("reset".into(), 1),
                ("run".into(), 0),
                ("jump_en".into(), 0),
                ("jump_addr".into(), 0),
            ]))
            .expect("reset");
        assert_eq!(outputs.get("pc"), Some(&0));

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
                ("jump_en".into(), 0),
                ("jump_addr".into(), 0),
            ]))
            .expect("increment");
        assert_eq!(outputs.get("pc"), Some(&1));

        let outputs = sim
            .step(BTreeMap::from([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
                ("jump_en".into(), 1),
                ("jump_addr".into(), 10),
            ]))
            .expect("jump");
        assert_eq!(outputs.get("pc"), Some(&10));
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
}
