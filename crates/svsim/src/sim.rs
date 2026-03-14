use std::collections::{BTreeMap, HashMap};

use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::hir::{
    BinaryOp, Expr, HirDesign, LValue, ModuleInstanceSummary, ModuleSummary, NumericLiteral,
    PortDirection, UnaryOp,
};

#[derive(Debug, Clone)]
pub struct SimulationSession {
    design: CompiledDesign,
}

impl SimulationSession {
    pub(crate) fn new(design: CompiledDesign) -> Self {
        Self { design }
    }

    pub fn top_module(&self) -> &str {
        self.design
            .top_module()
            .expect("compiled designs always carry a top module")
    }

    pub fn eval_once(&mut self, inputs: BTreeMap<String, u64>) -> Result<BTreeMap<String, u64>> {
        let mut stack = Vec::new();
        let values = evaluate_module(self.design.hir(), self.top_module(), &inputs, &mut stack)?;
        let module =
            self.design.hir().module(self.top_module()).ok_or_else(|| {
                Error::Resolve(format!("missing top module '{}'", self.top_module()))
            })?;

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

        Ok(outputs)
    }

    pub fn step(&mut self, _inputs: BTreeMap<String, u64>) -> Result<BTreeMap<String, u64>> {
        Err(Error::Unsupported(
            "the sequential engine is not implemented yet".into(),
        ))
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

fn evaluate_module(
    hir: &HirDesign,
    module_name: &str,
    inputs: &BTreeMap<String, u64>,
    stack: &mut Vec<String>,
) -> Result<HashMap<String, Value>> {
    if stack.iter().any(|name| name == module_name) {
        return Err(Error::Unsupported(format!(
            "recursive combinational instantiation detected at {}",
            stack.join(" -> ")
        )));
    }

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

    let mut values = build_signal_table(module, inputs)?;
    let max_iterations =
        ((module.continuous_assignments.len() + module.instantiations.len() + values.len()).max(1))
            * 8;

    stack.push(module_name.to_owned());
    let mut converged = false;
    for _ in 0..max_iterations {
        let mut changed = false;

        for assign in &module.continuous_assignments {
            let value = eval_expr(&assign.expr, &values)?;
            changed |= apply_lvalue(&assign.target, value, module, &mut values)?;
        }

        for instance in &module.instantiations {
            changed |= evaluate_instance(hir, module, instance, &mut values, stack)?;
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
            module_name
        )));
    }

    Ok(values)
}

fn build_signal_table(
    module: &ModuleSummary,
    inputs: &BTreeMap<String, u64>,
) -> Result<HashMap<String, Value>> {
    let mut values = HashMap::new();

    for port in &module.ports {
        let mut value = Value::zero(port.width());
        if matches!(port.direction, PortDirection::Input) {
            value = Value::new(*inputs.get(&port.name).unwrap_or(&0), port.width());
        }
        values.insert(port.name.clone(), value);
    }

    for signal in &module.signals {
        values.insert(signal.name.clone(), Value::zero(signal.width()));
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

fn evaluate_instance(
    hir: &HirDesign,
    parent: &ModuleSummary,
    instance: &ModuleInstanceSummary,
    values: &mut HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Result<bool> {
    let child = hir.module(&instance.module_name).ok_or_else(|| {
        Error::Resolve(format!(
            "instance '{}' references missing module '{}'",
            instance.instance_name, instance.module_name
        ))
    })?;

    let mut child_inputs = BTreeMap::new();
    for port in child
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input))
    {
        let Some(connection) = find_connection(instance, &port.name) else {
            continue;
        };
        let value = eval_expr(&connection.expr, values)?;
        child_inputs.insert(port.name.clone(), value.normalized_bits());
    }

    let child_values = evaluate_module(hir, &instance.module_name, &child_inputs, stack)?;
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
            let width = left.width.max(right.width);
            let bits = match op {
                BinaryOp::BitAnd => left.normalized_bits() & right.normalized_bits(),
                BinaryOp::BitOr => left.normalized_bits() | right.normalized_bits(),
                BinaryOp::BitXor => left.normalized_bits() ^ right.normalized_bits(),
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
    use std::path::PathBuf;

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
}
