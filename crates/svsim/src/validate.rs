use std::collections::HashSet;
use std::path::Path;

use crate::diag::{Error, Result};
use crate::hir::{
    AssignmentKind, BinaryOp, CaseStmtItem, Expr, HirDesign, LValue, MemoryDecl,
    ModuleInstanceSummary, ModuleSummary, NumericLiteral, PortDirection, ProcBlockKind, Stmt,
    UnaryOp,
};
use crate::width::minimum_width;

const MAX_RUNTIME_WIDTH: usize = u64::BITS as usize;

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
    validate_module(hir, module)?;

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

fn validate_module(hir: &HirDesign, module: &ModuleSummary) -> Result<()> {
    validate_supported_port_directions(module)?;
    validate_unique_declarations(module)?;
    validate_unique_instance_names(module)?;
    validate_runtime_widths(module)?;
    validate_legacy_rom_primitive(hir, module)?;

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

fn validate_runtime_widths(module: &ModuleSummary) -> Result<()> {
    for port in &module.ports {
        ensure_runtime_width(
            port.width(),
            format!("port '{}' in '{}'", port.name, module.name),
        )?;
    }

    for signal in &module.signals {
        ensure_runtime_width(
            signal.width(),
            format!("signal '{}' in '{}'", signal.name, module.name),
        )?;
    }

    for memory in &module.memories {
        ensure_runtime_width(
            memory.element_width(),
            format!("memory element '{}' in '{}'", memory.name, module.name),
        )?;
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

fn validate_unique_instance_names(module: &ModuleSummary) -> Result<()> {
    let mut names = HashSet::new();

    for instance in &module.instantiations {
        if !names.insert(instance.instance_name.as_str()) {
            return Err(Error::Resolve(format!(
                "module '{}' declares instance '{}' more than once",
                module.name, instance.instance_name
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
        Expr::Literal(literal) => ensure_runtime_width(
            literal.width.unwrap_or_else(|| minimum_width(literal.bits)),
            "literal",
        ),
        Expr::Concat(exprs) => {
            let mut width = 0usize;
            for expr in exprs {
                width = width.saturating_add(validate_expr(expr, module)?);
            }
            ensure_runtime_width(width, "concatenation expression")
        }
        Expr::Repeat { count, expr } => ensure_runtime_width(
            validate_expr(expr, module)?.saturating_mul(*count),
            "replication expression",
        ),
        Expr::MemoryRead { memory, index } => {
            validate_expr(index, module)?;
            let memory = module.memory_decl(memory).ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            })?;
            validate_constant_memory_index(index, memory, module)?;
            ensure_runtime_width(
                memory.element_width(),
                format!("memory element '{}' in '{}'", memory.name, module.name),
            )
        }
        Expr::BitSelect { expr, index } => {
            let width = validate_expr(expr, module)?;
            if *index >= width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for width {}",
                    index, width
                )));
            }
            ensure_runtime_width(1, "bit-select expression")
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
            ensure_runtime_width(high - (*msb).min(*lsb) + 1, "part-select expression")
        }
        Expr::Unary { expr, .. } => validate_expr(expr, module),
        Expr::Binary { left, op, right } => {
            let left_width = validate_expr(left, module)?;
            let right_width = validate_expr(right, module)?;
            ensure_runtime_width(
                match op {
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::Eq | BinaryOp::NotEq => {
                        1
                    }
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Add
                    | BinaryOp::Sub => left_width.max(right_width),
                },
                "binary expression",
            )
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            validate_expr(cond, module)?;
            let when_true_width = validate_expr(when_true, module)?;
            let when_false_width = validate_expr(when_false, module)?;
            ensure_runtime_width(when_true_width.max(when_false_width), "ternary expression")
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
            ensure_runtime_width(width, "concatenated assignment target")
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
            let memory = module.memory_decl(memory).ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            })?;
            validate_constant_memory_index(index, memory, module)?;
            Ok(memory.element_width())
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

fn validate_constant_memory_index(
    index: &Expr,
    memory: &MemoryDecl,
    module: &ModuleSummary,
) -> Result<()> {
    let Some(index) = const_eval_expr(index) else {
        return Ok(());
    };
    let raw_index = index.normalized_bits();
    let index = usize::try_from(raw_index).map_err(|_| {
        Error::Resolve(format!(
            "memory index [{}] is out of range for '{}' in '{}'",
            raw_index, memory.name, module.name
        ))
    })?;

    if memory.index_range.contains_index(index) {
        Ok(())
    } else {
        Err(Error::Resolve(format!(
            "memory index [{}] is out of range for '{}' in '{}'",
            raw_index, memory.name, module.name
        )))
    }
}

#[derive(Debug, Clone, Copy)]
struct ConstValue {
    bits: u64,
    width: usize,
}

impl ConstValue {
    fn new(bits: u64, width: usize) -> Self {
        Self {
            bits: bits & mask(width),
            width,
        }
    }

    fn normalized_bits(self) -> u64 {
        self.bits & mask(self.width)
    }

    fn truthy(self) -> bool {
        self.normalized_bits() != 0
    }
}

fn const_eval_expr(expr: &Expr) -> Option<ConstValue> {
    match expr {
        Expr::Ident(_) | Expr::MemoryRead { .. } => None,
        Expr::Literal(literal) => Some(const_value_from_literal(literal)),
        Expr::Concat(exprs) => {
            let mut parts = Vec::with_capacity(exprs.len());
            for expr in exprs {
                parts.push(const_eval_expr(expr)?);
            }
            concat_const_values(&parts)
        }
        Expr::Repeat { count, expr } => {
            let value = const_eval_expr(expr)?;
            let values = vec![value; *count];
            concat_const_values(&values)
        }
        Expr::BitSelect { expr, index } => {
            let value = const_eval_expr(expr)?;
            Some(ConstValue::new((value.normalized_bits() >> index) & 1, 1))
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let value = const_eval_expr(expr)?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            let width = high - low + 1;
            Some(ConstValue::new(
                (value.normalized_bits() >> low) & mask(width),
                width,
            ))
        }
        Expr::Unary { op, expr } => {
            let value = const_eval_expr(expr)?;
            match op {
                UnaryOp::BitNot => Some(ConstValue::new(!value.normalized_bits(), value.width)),
            }
        }
        Expr::Binary { left, op, right } => {
            let left = const_eval_expr(left)?;
            let right = const_eval_expr(right)?;
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
                BinaryOp::Eq => (
                    (left.normalized_bits() == right.normalized_bits()) as u64,
                    1,
                ),
                BinaryOp::NotEq => (
                    (left.normalized_bits() != right.normalized_bits()) as u64,
                    1,
                ),
                BinaryOp::Add => (
                    left.normalized_bits().wrapping_add(right.normalized_bits()),
                    left.width.max(right.width),
                ),
                BinaryOp::Sub => (
                    left.normalized_bits().wrapping_sub(right.normalized_bits()),
                    left.width.max(right.width),
                ),
            };
            Some(ConstValue::new(bits, width))
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            let cond = const_eval_expr(cond)?;
            let when_true = const_eval_expr(when_true)?;
            let when_false = const_eval_expr(when_false)?;
            let result_width = when_true.width.max(when_false.width);
            let value = if cond.truthy() {
                when_true.normalized_bits()
            } else {
                when_false.normalized_bits()
            };
            Some(ConstValue::new(value, result_width))
        }
    }
}

fn const_value_from_literal(literal: &NumericLiteral) -> ConstValue {
    let width = literal.width.unwrap_or_else(|| minimum_width(literal.bits));
    ConstValue::new(literal.bits, width)
}

fn concat_const_values(parts: &[ConstValue]) -> Option<ConstValue> {
    let total_width: usize = parts.iter().map(|value| value.width).sum();
    if total_width == 0 || total_width > MAX_RUNTIME_WIDTH {
        return None;
    }

    let mut bits = 0u64;
    let mut shift = total_width;
    for part in parts {
        shift -= part.width;
        bits |= (part.normalized_bits() & mask(part.width)) << shift;
    }

    Some(ConstValue::new(bits, total_width))
}

fn mask(width: usize) -> u64 {
    if width >= u64::BITS as usize {
        u64::MAX
    } else {
        (1u64 << width) - 1
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

fn ensure_runtime_width(width: usize, context: impl Into<String>) -> Result<usize> {
    let context = context.into();

    if width == 0 {
        return Err(Error::Unsupported(format!(
            "{} has width 0 outside the supported 1..={} bit runtime subset",
            context, MAX_RUNTIME_WIDTH
        )));
    }

    if width > MAX_RUNTIME_WIDTH {
        return Err(Error::Unsupported(format!(
            "{} has width {} exceeding the current {}-bit runtime limit",
            context, width, MAX_RUNTIME_WIDTH
        )));
    }

    Ok(width)
}

fn validate_legacy_rom_primitive(hir: &HirDesign, module: &ModuleSummary) -> Result<()> {
    if !module.name.starts_with("rom_") {
        return Ok(());
    }

    let rom_name = &module.name["rom_".len()..];
    if rom_name.is_empty() {
        return Err(Error::Unsupported(
            "legacy ROM primitive names must include a non-empty file stem after `rom_`".into(),
        ));
    }

    if !module.signals.is_empty()
        || !module.memories.is_empty()
        || !module.continuous_assignments.is_empty()
        || !module.proc_blocks.is_empty()
        || !module.instantiations.is_empty()
    {
        return Err(Error::Unsupported(format!(
            "legacy ROM primitive '{}' must be a port-only wrapper with no internal declarations or logic",
            module.name
        )));
    }

    let input_ports = module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input))
        .collect::<Vec<_>>();
    let output_ports = module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output))
        .collect::<Vec<_>>();
    if module.ports.len() != 2 || input_ports.len() != 1 || output_ports.len() != 1 {
        return Err(Error::Unsupported(format!(
            "legacy ROM primitive '{}' must declare exactly one input address port and one output data port",
            module.name
        )));
    }

    let source_path = hir.module_source_path(&module.name).ok_or_else(|| {
        Error::Resolve(format!(
            "could not determine source file for legacy ROM primitive '{}'",
            module.name
        ))
    })?;
    resolve_legacy_rom_data_path(source_path, rom_name).ok_or_else(|| {
        Error::Resolve(format!(
            "legacy ROM primitive '{}' could not find '{}.txt'",
            module.name, rom_name
        ))
    })?;

    let addr_port = input_ports[0];
    let _ = 1usize
        .checked_shl(addr_port.width() as u32)
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "legacy ROM primitive '{}' address width {} exceeds host limits",
                module.name,
                addr_port.width()
            ))
        })?;

    Ok(())
}

fn resolve_legacy_rom_data_path(source_path: &Path, rom_name: &str) -> Option<std::path::PathBuf> {
    let file_name = format!("{rom_name}.txt");
    let mut candidates = Vec::new();
    if let Some(source_dir) = source_path.parent() {
        candidates.push(source_dir.join(&file_name));
        candidates.push(source_dir.join("roms").join(&file_name));
    }
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("roms").join(&file_name));
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}
