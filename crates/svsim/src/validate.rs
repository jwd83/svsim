use std::collections::HashSet;

use crate::diag::{Error, Result};
use crate::hir::{
    AssignmentKind, BinaryOp, CaseStmtItem, Expr, HirDesign, LValue, ModuleInstanceSummary,
    ModuleSummary, PortDirection, ProcBlockKind, Stmt,
};

pub(crate) fn validate_design(hir: &HirDesign) -> Result<()> {
    let mut validated = HashSet::new();
    let mut stack = Vec::new();

    for file in hir.files() {
        for module in &file.modules {
            validate_module_recursive(hir, &module.name, &mut stack, &mut validated)?;
        }
    }

    Ok(())
}

fn validate_module_recursive(
    hir: &HirDesign,
    module_name: &str,
    stack: &mut Vec<String>,
    validated: &mut HashSet<String>,
) -> Result<()> {
    if validated.contains(module_name) {
        return Ok(());
    }

    if stack.iter().any(|name| name == module_name) {
        return Err(Error::Resolve(format!(
            "recursive instantiation detected at {} -> {}",
            stack.join(" -> "),
            module_name
        )));
    }

    let module = hir
        .module(module_name)
        .ok_or_else(|| Error::Resolve(format!("module '{}' was not compiled", module_name)))?;
    validate_module(module)?;

    stack.push(module_name.to_owned());
    for instance in &module.instantiations {
        let child = hir.module(&instance.module_name).ok_or_else(|| {
            Error::Resolve(format!(
                "instance '{}' references missing module '{}'",
                instance.instance_name, instance.module_name
            ))
        })?;
        validate_instance(module, instance, child)?;
        validate_module_recursive(hir, &instance.module_name, stack, validated)?;
    }
    stack.pop();

    validated.insert(module_name.to_owned());
    Ok(())
}

fn validate_module(module: &ModuleSummary) -> Result<()> {
    validate_supported_port_directions(module)?;
    validate_unique_declarations(module)?;

    for assign in &module.continuous_assignments {
        validate_expr(&assign.expr, module)?;
        validate_assignment_target(&assign.target, module)?;
        if lvalue_contains_memory(&assign.target) {
            return Err(Error::Unsupported(
                "continuous assignments to memory elements are not supported".into(),
            ));
        }
    }

    for block in &module.proc_blocks {
        if let ProcBlockKind::AlwaysFf { clock } = &block.kind {
            if module.signal_width(clock).is_none() {
                return Err(Error::Resolve(format!(
                    "clock '{}' is not declared in '{}'",
                    clock, module.name
                )));
            }
        }
        validate_stmt(&block.body, module, &block.kind)?;
    }

    Ok(())
}

fn validate_unique_declarations(module: &ModuleSummary) -> Result<()> {
    let mut names = HashSet::new();

    for port in &module.ports {
        if !names.insert(port.name.as_str()) {
            return Err(Error::Resolve(format!(
                "module '{}' declares '{}' more than once",
                module.name, port.name
            )));
        }
    }

    for signal in &module.signals {
        if !names.insert(signal.name.as_str()) {
            return Err(Error::Resolve(format!(
                "module '{}' declares '{}' more than once",
                module.name, signal.name
            )));
        }
    }

    for memory in &module.memories {
        if !names.insert(memory.name.as_str()) {
            return Err(Error::Resolve(format!(
                "module '{}' declares '{}' more than once",
                module.name, memory.name
            )));
        }
    }

    Ok(())
}

fn validate_supported_port_directions(module: &ModuleSummary) -> Result<()> {
    for port in &module.ports {
        match port.direction {
            PortDirection::Input | PortDirection::Output => {}
            PortDirection::Inout => {
                return Err(Error::Unsupported(format!(
                    "module '{}' uses unsupported `inout` port '{}'",
                    module.name, port.name
                )));
            }
            PortDirection::Ref => {
                return Err(Error::Unsupported(format!(
                    "module '{}' uses unsupported `ref` port '{}'",
                    module.name, port.name
                )));
            }
        }
    }

    Ok(())
}

fn validate_instance(
    parent: &ModuleSummary,
    instance: &ModuleInstanceSummary,
    child: &ModuleSummary,
) -> Result<()> {
    let mut connected_ports = HashSet::new();

    for connection in &instance.connections {
        if !connected_ports.insert(connection.port_name.as_str()) {
            return Err(Error::Resolve(format!(
                "instance '{}' connects port '{}' more than once",
                instance.instance_name, connection.port_name
            )));
        }

        let port = child.port(&connection.port_name).ok_or_else(|| {
            Error::Resolve(format!(
                "instance '{}' connects unknown port '{}' on module '{}'",
                instance.instance_name, connection.port_name, child.name
            ))
        })?;

        validate_expr(&connection.expr, parent)?;
        if matches!(port.direction, PortDirection::Output) {
            let lvalue = expr_to_lvalue(&connection.expr).ok_or_else(|| {
                Error::Unsupported(format!(
                    "instance '{}' connects output port '{}' to a non-lvalue expression",
                    instance.instance_name, port.name
                ))
            })?;
            validate_assignment_target(&lvalue, parent)?;
            if lvalue_contains_memory(&lvalue) {
                return Err(Error::Unsupported(format!(
                    "instance '{}' connects output port '{}' to a memory element, which is not supported",
                    instance.instance_name, port.name
                )));
            }
        }
    }

    for port in child
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input))
    {
        if !connected_ports.contains(port.name.as_str()) {
            return Err(Error::Resolve(format!(
                "instance '{}' is missing a connection for input port '{}' on module '{}'",
                instance.instance_name, port.name, child.name
            )));
        }
    }

    Ok(())
}

fn validate_stmt(stmt: &Stmt, module: &ModuleSummary, block_kind: &ProcBlockKind) -> Result<()> {
    match stmt {
        Stmt::Empty => Ok(()),
        Stmt::Block(statements) => {
            for statement in statements {
                validate_stmt(statement, module, block_kind)?;
            }
            Ok(())
        }
        Stmt::Assign { kind, target, expr } => {
            validate_expr(expr, module)?;
            validate_assignment_target(target, module)?;

            if lvalue_contains_memory(target) && matches!(block_kind, ProcBlockKind::AlwaysComb) {
                return Err(Error::Unsupported(
                    "memory element assignments are only supported inside `always_ff` blocks"
                        .into(),
                ));
            }

            if matches!(block_kind, ProcBlockKind::AlwaysComb)
                && matches!(kind, AssignmentKind::Nonblocking)
            {
                return Err(Error::Unsupported(
                    "nonblocking assignments are only supported inside `always_ff` blocks".into(),
                ));
            }

            Ok(())
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            validate_expr(cond, module)?;
            validate_stmt(then_branch, module, block_kind)?;
            if let Some(else_branch) = else_branch {
                validate_stmt(else_branch, module, block_kind)?;
            }
            Ok(())
        }
        Stmt::Case {
            expr,
            items,
            default,
        } => {
            validate_expr(expr, module)?;
            for item in items {
                validate_case_item(item, module, block_kind)?;
            }
            if let Some(default) = default {
                validate_stmt(default, module, block_kind)?;
            }
            Ok(())
        }
    }
}

fn validate_case_item(
    item: &CaseStmtItem,
    module: &ModuleSummary,
    block_kind: &ProcBlockKind,
) -> Result<()> {
    for pattern in &item.patterns {
        validate_expr(pattern, module)?;
    }
    validate_stmt(&item.body, module, block_kind)
}

fn validate_expr(expr: &Expr, module: &ModuleSummary) -> Result<usize> {
    match expr {
        Expr::Ident(name) => module.signal_width(name).ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                name, module.name
            ))
        }),
        Expr::Literal(literal) => Ok(literal.width.unwrap_or_else(|| minimum_width(literal.bits))),
        Expr::Concat(exprs) => {
            let mut width = 0usize;
            for expr in exprs {
                width = width.saturating_add(validate_expr(expr, module)?);
            }
            Ok(width)
        }
        Expr::Repeat { count, expr } => Ok(validate_expr(expr, module)?.saturating_mul(*count)),
        Expr::MemoryRead { memory, index } => {
            validate_expr(index, module)?;
            module
                .memory_decl(memory)
                .map(|memory| memory.element_width())
                .ok_or_else(|| {
                    Error::Resolve(format!(
                        "memory '{}' is not declared in '{}'",
                        memory, module.name
                    ))
                })
        }
        Expr::BitSelect { expr, index } => {
            let width = validate_expr(expr, module)?;
            if *index >= width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for width {}",
                    index, width
                )));
            }
            Ok(1)
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let width = validate_expr(expr, module)?;
            let high = (*msb).max(*lsb);
            if high >= width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for width {}",
                    msb, lsb, width
                )));
            }
            Ok(high - (*msb).min(*lsb) + 1)
        }
        Expr::Unary { expr, .. } => validate_expr(expr, module),
        Expr::Binary { left, op, right } => {
            let left_width = validate_expr(left, module)?;
            let right_width = validate_expr(right, module)?;
            Ok(match op {
                BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::Eq | BinaryOp::NotEq => 1,
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Add
                | BinaryOp::Sub => left_width.max(right_width),
            })
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            validate_expr(cond, module)?;
            let when_true_width = validate_expr(when_true, module)?;
            let when_false_width = validate_expr(when_false, module)?;
            Ok(when_true_width.max(when_false_width))
        }
    }
}

fn validate_lvalue(lvalue: &LValue, module: &ModuleSummary) -> Result<usize> {
    match lvalue {
        LValue::Signal(name) => module.signal_width(name).ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                name, module.name
            ))
        }),
        LValue::Concat(items) => {
            let mut width = 0usize;
            for item in items {
                width = width.saturating_add(validate_lvalue(item, module)?);
            }
            Ok(width)
        }
        LValue::BitSelect { signal, index } => {
            let width = module.signal_width(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            if *index >= width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for signal '{}'",
                    index, signal
                )));
            }
            Ok(1)
        }
        LValue::PartSelect { signal, msb, lsb } => {
            let width = module.signal_width(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            let high = (*msb).max(*lsb);
            if high >= width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for signal '{}'",
                    msb, lsb, signal
                )));
            }
            Ok(high - (*msb).min(*lsb) + 1)
        }
        LValue::MemoryElement { memory, index } => {
            validate_expr(index, module)?;
            module
                .memory_decl(memory)
                .map(|memory| memory.element_width())
                .ok_or_else(|| {
                    Error::Resolve(format!(
                        "memory '{}' is not declared in '{}'",
                        memory, module.name
                    ))
                })
        }
    }
}

fn validate_assignment_target(lvalue: &LValue, module: &ModuleSummary) -> Result<usize> {
    let width = validate_lvalue(lvalue, module)?;
    validate_port_drive_targets(lvalue, module)?;
    Ok(width)
}

fn validate_port_drive_targets(lvalue: &LValue, module: &ModuleSummary) -> Result<()> {
    match lvalue {
        LValue::Signal(name) => validate_driven_signal(name, module),
        LValue::Concat(items) => {
            for item in items {
                validate_port_drive_targets(item, module)?;
            }
            Ok(())
        }
        LValue::BitSelect { signal, .. } | LValue::PartSelect { signal, .. } => {
            validate_driven_signal(signal, module)
        }
        LValue::MemoryElement { .. } => Ok(()),
    }
}

fn validate_driven_signal(signal: &str, module: &ModuleSummary) -> Result<()> {
    if let Some(port) = module.port(signal) {
        if matches!(port.direction, PortDirection::Input) {
            return Err(Error::Resolve(format!(
                "input port '{}' in '{}' cannot be driven",
                signal, module.name
            )));
        }
    }

    Ok(())
}

fn lvalue_contains_memory(lvalue: &LValue) -> bool {
    match lvalue {
        LValue::Signal(_) | LValue::BitSelect { .. } | LValue::PartSelect { .. } => false,
        LValue::Concat(items) => items.iter().any(lvalue_contains_memory),
        LValue::MemoryElement { .. } => true,
    }
}

fn expr_to_lvalue(expr: &Expr) -> Option<LValue> {
    match expr {
        Expr::Ident(name) => Some(LValue::Signal(name.clone())),
        Expr::Concat(exprs) => {
            let mut items = Vec::with_capacity(exprs.len());
            for expr in exprs {
                items.push(expr_to_lvalue(expr)?);
            }
            Some(LValue::Concat(items))
        }
        Expr::BitSelect { expr, index } => match expr.as_ref() {
            Expr::Ident(signal) => Some(LValue::BitSelect {
                signal: signal.clone(),
                index: *index,
            }),
            _ => None,
        },
        Expr::PartSelect { expr, msb, lsb } => match expr.as_ref() {
            Expr::Ident(signal) => Some(LValue::PartSelect {
                signal: signal.clone(),
                msb: *msb,
                lsb: *lsb,
            }),
            _ => None,
        },
        Expr::MemoryRead { memory, index } => Some(LValue::MemoryElement {
            memory: memory.clone(),
            index: index.clone(),
        }),
        Expr::Literal(_)
        | Expr::Repeat { .. }
        | Expr::Unary { .. }
        | Expr::Binary { .. }
        | Expr::Ternary { .. } => None,
    }
}

fn minimum_width(bits: u64) -> usize {
    if bits == 0 {
        1
    } else {
        (u64::BITS - bits.leading_zeros()) as usize
    }
}
