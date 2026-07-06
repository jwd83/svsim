//! Expression evaluation, lvalue resolution, and net-driver staging.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResolvedLValue {
    Signal(String),
    Concat(Vec<ResolvedLValue>),
    BitSelect {
        signal: String,
        index: usize,
    },
    PartSelect {
        signal: String,
        msb: usize,
        lsb: usize,
    },
    MemoryElement {
        memory: String,
        index: usize,
    },
}

pub(super) fn resolve_supported_module<'a>(
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

pub(super) fn eval_expr(
    expr: &Expr,
    module: &ModuleSummary,
    values: &HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<Value> {
    match expr {
        Expr::Ident(name) => values
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Resolve(format!("signal '{}' is not declared", name))),
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        Expr::Concat(exprs) => {
            let mut values_out = Vec::with_capacity(exprs.len());
            for expr in exprs {
                values_out.push(eval_expr(expr, module, values, memories)?);
            }
            concat_values(&values_out)
        }
        Expr::Repeat { count, expr } => {
            let value = eval_expr(expr, module, values, memories)?;
            let values_out = vec![value; *count];
            concat_values(&values_out)
        }
        Expr::MemoryRead { memory, index } => {
            let index_value = eval_expr(index, module, values, memories)?;
            let index = index_value
                .to_bit_value_checked()
                .and_then(|bits| bits.to_usize_checked())
                .ok_or_else(|| Error::Resolve("memory index exceeds host limits".into()))?;
            let memory_state = memories
                .get(memory)
                .ok_or_else(|| Error::Resolve(format!("memory '{}' is not declared", memory)))?;
            memory_state.read(index, memory)
        }
        Expr::BitSelect { expr, index } => {
            let value = eval_expr(expr, module, values, memories)?;
            if *index >= value.width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for width {}",
                    index, value.width
                )));
            }
            Ok(Value::from_logic(
                logic_value_from_bit(value.logic.bit(*index)),
                1,
            ))
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let value = eval_expr(expr, module, values, memories)?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            if high >= value.width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for width {}",
                    msb, lsb, value.width
                )));
            }
            let width = high - low + 1;
            Ok(Value::from_logic(
                logic_slice(value.logic(), low, width),
                width,
            ))
        }
        Expr::Unary { op, expr } => {
            let value = eval_expr(expr, module, values, memories)?;
            match op {
                UnaryOp::BitNot => {
                    let mut bits = LogicBits::zero();
                    for index in 0..value.width {
                        bits.set_bit(index, logic_bit_not(value.logic.bit(index)));
                    }
                    Ok(Value::from_logic_with_signed(
                        LogicValue::new(bits, value.width),
                        value.signed,
                    ))
                }
                UnaryOp::Negate => {
                    if let Some(bits) = value.to_bit_value_checked() {
                        Ok(Value::new_with_signed(
                            BitValue::zero().wrapping_sub(&bits, value.width),
                            value.width,
                            value.signed,
                        ))
                    } else {
                        Ok(Value::unknown(value.width))
                    }
                }
                UnaryOp::LogicalNot => Ok(Value::from_logic(
                    logic_value_from_bit(match value.truthiness() {
                        LogicTruth::False => LogicBit::One,
                        LogicTruth::True => LogicBit::Zero,
                        LogicTruth::Unknown => LogicBit::X,
                    }),
                    1,
                )),
                UnaryOp::ReductionOr => Ok(Value::from_logic(
                    logic_value_from_bit(logic_reduce_or(&value)),
                    1,
                )),
                UnaryOp::ReductionAnd => Ok(Value::from_logic(
                    logic_value_from_bit(logic_reduce_and(&value)),
                    1,
                )),
                UnaryOp::ReductionNand => Ok(Value::from_logic(
                    logic_value_from_bit(logic_bit_not(logic_reduce_and(&value))),
                    1,
                )),
                UnaryOp::ReductionXor => Ok(Value::from_logic(
                    logic_value_from_bit(logic_reduce_xor(&value)),
                    1,
                )),
                UnaryOp::Signed => Ok(Value::from_logic_with_signed(value.logic, true)),
                UnaryOp::Unsigned => Ok(Value::from_logic_with_signed(value.logic, false)),
            }
        }
        Expr::Binary { left, op, right } => {
            let mut left = eval_expr(left, module, values, memories)?;
            let mut right = eval_expr(right, module, values, memories)?;
            let common_width = left.width.max(right.width);
            left = left.coerced_to(common_width);
            right = right.coerced_to(common_width);
            match op {
                BinaryOp::BitAnd => Ok(logic_bitwise_binary(&left, &right, logic_bit_and)),
                BinaryOp::BitOr => Ok(logic_bitwise_binary(&left, &right, logic_bit_or)),
                BinaryOp::BitXor => Ok(logic_bitwise_binary(&left, &right, logic_bit_xor)),
                BinaryOp::ShiftLeft => Ok(logic_shift_left_value(&left, &right)),
                BinaryOp::ShiftRight => Ok(logic_shift_right_value(&left, &right)),
                BinaryOp::ArithmeticShiftRight => {
                    Ok(logic_arithmetic_shift_right_value(&left, &right))
                }
                BinaryOp::LogicalAnd => Ok(Value::from_logic(
                    logic_value_from_bit(logical_and(&left, &right)),
                    1,
                )),
                BinaryOp::LogicalOr => Ok(Value::from_logic(
                    logic_value_from_bit(logical_or(&left, &right)),
                    1,
                )),
                BinaryOp::Eq => Ok(Value::from_logic(
                    logic_value_from_bit(values_logical_equal(&left, &right)),
                    1,
                )),
                BinaryOp::NotEq => Ok(Value::from_logic(
                    logic_value_from_bit(logic_bit_not(values_logical_equal(&left, &right))),
                    1,
                )),
                BinaryOp::Lt => Ok(logic_value_from_ordering(
                    compare_values(&left, &right).map(|ordering| ordering.is_lt()),
                )),
                BinaryOp::LtEq => Ok(logic_value_from_ordering(
                    compare_values(&left, &right).map(|ordering| !ordering.is_gt()),
                )),
                BinaryOp::Gt => Ok(logic_value_from_ordering(
                    compare_values(&left, &right).map(|ordering| ordering.is_gt()),
                )),
                BinaryOp::GtEq => Ok(logic_value_from_ordering(
                    compare_values(&left, &right).map(|ordering| !ordering.is_lt()),
                )),
                BinaryOp::Add => {
                    if let (Some(left_bits), Some(right_bits)) =
                        (left.to_bit_value_checked(), right.to_bit_value_checked())
                    {
                        Ok(Value::new_with_signed(
                            left_bits.wrapping_add(&right_bits, common_width),
                            common_width,
                            left.signed && right.signed,
                        ))
                    } else {
                        Ok(Value::unknown(common_width))
                    }
                }
                BinaryOp::Sub => {
                    if let (Some(left_bits), Some(right_bits)) =
                        (left.to_bit_value_checked(), right.to_bit_value_checked())
                    {
                        Ok(Value::new_with_signed(
                            left_bits.wrapping_sub(&right_bits, common_width),
                            common_width,
                            left.signed && right.signed,
                        ))
                    } else {
                        Ok(Value::unknown(common_width))
                    }
                }
                BinaryOp::Mul => {
                    if let (Some(left_bits), Some(right_bits)) =
                        (left.to_bit_value_checked(), right.to_bit_value_checked())
                    {
                        Ok(Value::new_with_signed(
                            left_bits.wrapping_mul(&right_bits, common_width),
                            common_width,
                            left.signed && right.signed,
                        ))
                    } else {
                        Ok(Value::unknown(common_width))
                    }
                }
            }
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            let result_width = expr_width(when_true, module)?.max(expr_width(when_false, module)?);
            let condition = eval_expr(cond, module, values, memories)?;
            let when_true = eval_expr(when_true, module, values, memories)?;
            let when_false = eval_expr(when_false, module, values, memories)?;
            match condition.truthiness() {
                LogicTruth::True => Ok(when_true.coerced_to(result_width)),
                LogicTruth::False => Ok(when_false.coerced_to(result_width)),
                LogicTruth::Unknown => {
                    Ok(logic_ternary_merge(&when_true, &when_false, result_width))
                }
            }
        }
    }
}

pub(super) fn resolve_lvalue(
    lvalue: &LValue,
    module: &ModuleSummary,
    values: &HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<ResolvedLValue> {
    match lvalue {
        LValue::Signal(name) => {
            if module.signal_width(name).is_none() {
                return Err(Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                )));
            }
            Ok(ResolvedLValue::Signal(name.clone()))
        }
        LValue::Concat(items) => {
            let mut resolved = Vec::with_capacity(items.len());
            for item in items {
                resolved.push(resolve_lvalue(item, module, values, memories)?);
            }
            Ok(ResolvedLValue::Concat(resolved))
        }
        LValue::BitSelect { signal, index } => Ok(ResolvedLValue::BitSelect {
            signal: signal.clone(),
            index: *index,
        }),
        LValue::PartSelect { signal, msb, lsb } => Ok(ResolvedLValue::PartSelect {
            signal: signal.clone(),
            msb: *msb,
            lsb: *lsb,
        }),
        LValue::MemoryElement { memory, index } => Ok(ResolvedLValue::MemoryElement {
            memory: memory.clone(),
            index: eval_expr(index, module, values, memories)?
                .to_bit_value_checked()
                .and_then(|bits| bits.to_usize_checked())
                .ok_or_else(|| Error::Resolve("memory index exceeds host limits".into()))?,
        }),
    }
}

pub(super) fn resolved_lvalue_contains_memory(lvalue: &ResolvedLValue) -> bool {
    match lvalue {
        ResolvedLValue::Signal(_)
        | ResolvedLValue::BitSelect { .. }
        | ResolvedLValue::PartSelect { .. } => false,
        ResolvedLValue::Concat(items) => items.iter().any(resolved_lvalue_contains_memory),
        ResolvedLValue::MemoryElement { .. } => true,
    }
}

pub(super) fn resolved_lvalue_width(
    lvalue: &ResolvedLValue,
    module: &ModuleSummary,
) -> Result<usize> {
    match lvalue {
        ResolvedLValue::Signal(name) => module.signal_width(name).ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                name, module.name
            ))
        }),
        ResolvedLValue::Concat(items) => {
            let mut total = 0usize;
            for item in items {
                total += resolved_lvalue_width(item, module)?;
            }
            Ok(total)
        }
        ResolvedLValue::BitSelect { signal, index } => {
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
        ResolvedLValue::PartSelect { signal, msb, lsb } => {
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
        ResolvedLValue::MemoryElement { memory, .. } => module
            .memory_decl(memory)
            .map(|memory| memory.element_width())
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            }),
    }
}

pub(super) fn apply_resolved_lvalue(
    lvalue: &ResolvedLValue,
    value: Value,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
    memories: &mut HashMap<String, MemoryState>,
) -> Result<bool> {
    match lvalue {
        ResolvedLValue::Signal(name) => {
            let current = values.get_mut(name).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                ))
            })?;
            let coerced = value.coerced_to(current.width);
            let next = Value::from_logic(coerced.logic, current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::Concat(items) => {
            let total_width = resolved_lvalue_width(lvalue, module)?;
            let normalized = value.coerced_to(total_width);
            let mut remaining_width = total_width;
            let mut changed = false;
            for item in items {
                let item_width = resolved_lvalue_width(item, module)?;
                remaining_width -= item_width;
                let chunk = Value::from_logic(
                    logic_slice(normalized.logic(), remaining_width, item_width),
                    item_width,
                );
                changed |= apply_resolved_lvalue(item, chunk, module, values, memories)?;
            }
            Ok(changed)
        }
        ResolvedLValue::BitSelect { signal, index } => {
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
            let bit = value.coerced_to(1).logic().bit(0);
            let next = Value::from_logic(
                logic_replace_slice(current.logic(), *index, 1, &logic_value_from_bit(bit)),
                current.width,
            );
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::PartSelect { signal, msb, lsb } => {
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
            let next = Value::from_logic(
                logic_replace_slice(current.logic(), low, width, value.coerced_to(width).logic()),
                current.width,
            );
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::MemoryElement { memory, index } => {
            let memory_state = memories.get_mut(memory).ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            })?;
            memory_state.write(*index, value, memory)
        }
    }
}

pub(super) fn stage_signal_driver_if_net(
    signal_name: &str,
    value: Value,
    module: &ModuleSummary,
    state: &ModuleState,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    if !signal_storage(module, signal_name).is_some_and(StorageKind::is_net) {
        return Ok(());
    }
    let binding = state.signals.get(signal_name).copied().ok_or_else(|| {
        Error::Resolve(format!(
            "signal '{}' is not declared in '{}'",
            signal_name, module.name
        ))
    })?;
    stage_whole_signal_driver(binding, value, object_layouts, net_drivers)
}

pub(super) fn stage_whole_signal_driver(
    binding: SignalBinding,
    value: Value,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let logic = value.coerced_to(binding.view_width).logic;
    stage_whole_signal_logic_driver(binding, logic, object_layouts, net_drivers)
}

pub(super) fn stage_whole_signal_logic_driver(
    binding: SignalBinding,
    value: LogicValue,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let object = object_layouts.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    let logic = value
        .coerced_to(binding.view_width)
        .coerced_to(object.width);
    stage_object_driver(binding.object_id, logic, net_drivers);
    Ok(())
}

pub(super) fn stage_partial_signal_driver(
    binding: SignalBinding,
    low: usize,
    width: usize,
    value: Value,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let logic = value.coerced_to(width).logic;
    stage_partial_signal_logic_driver(binding, low, width, logic, object_layouts, net_drivers)
}

pub(super) fn stage_partial_signal_logic_driver(
    binding: SignalBinding,
    low: usize,
    width: usize,
    value: LogicValue,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let object = object_layouts.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    let mut bits = LogicBits::filled(object.width, LogicBit::Z);
    let value = value.coerced_to(width);
    for offset in 0..width {
        bits.set_bit(low + offset, value.bit(offset));
    }
    stage_object_driver(
        binding.object_id,
        LogicValue::new(bits, object.width),
        net_drivers,
    );
    Ok(())
}

pub(super) fn apply_or_stage_resolved_lvalue(
    lvalue: &ResolvedLValue,
    value: Value,
    module: &ModuleSummary,
    state: &ModuleState,
    values: &mut HashMap<String, Value>,
    memories: &mut HashMap<String, MemoryState>,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
    match lvalue {
        ResolvedLValue::Signal(name)
            if signal_storage(module, name).is_some_and(StorageKind::is_net) =>
        {
            let binding = state.signals.get(name).copied().ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                ))
            })?;
            stage_whole_signal_driver(binding, value, object_layouts, net_drivers)?;
            Ok(false)
        }
        ResolvedLValue::BitSelect { signal, index }
            if signal_storage(module, signal).is_some_and(StorageKind::is_net) =>
        {
            let binding = state.signals.get(signal).copied().ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            stage_partial_signal_driver(
                binding,
                *index,
                1,
                value.coerced_to(1),
                object_layouts,
                net_drivers,
            )?;
            Ok(false)
        }
        ResolvedLValue::PartSelect { signal, msb, lsb }
            if signal_storage(module, signal).is_some_and(StorageKind::is_net) =>
        {
            let binding = state.signals.get(signal).copied().ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            let low = (*msb).min(*lsb);
            let width = (*msb).max(*lsb) - low + 1;
            stage_partial_signal_driver(
                binding,
                low,
                width,
                value.coerced_to(width),
                object_layouts,
                net_drivers,
            )?;
            Ok(false)
        }
        ResolvedLValue::Concat(items) => {
            let total_width = resolved_lvalue_width(lvalue, module)?;
            let normalized = value.coerced_to(total_width);
            let mut remaining_width = total_width;
            let mut changed = false;
            for item in items {
                let item_width = resolved_lvalue_width(item, module)?;
                remaining_width -= item_width;
                let chunk = Value::from_logic(
                    logic_slice(normalized.logic(), remaining_width, item_width),
                    item_width,
                );
                changed |= apply_or_stage_resolved_lvalue(
                    item,
                    chunk,
                    module,
                    state,
                    values,
                    memories,
                    object_layouts,
                    net_drivers,
                )?;
            }
            Ok(changed)
        }
        _ => apply_resolved_lvalue(lvalue, value, module, values, memories),
    }
}

pub(super) fn resolve_staged_nets(
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &NetDriverTable,
) -> Result<bool> {
    let mut changed = false;

    for (object_id, object) in object_layouts.iter().enumerate() {
        let Some(kind) = object.storage.net_kind() else {
            continue;
        };
        let previous = frame.get(object_id).cloned().ok_or_else(|| {
            Error::Resolve(format!("runtime object {} has no value slot", object_id))
        })?;
        let resolved = resolve_net(
            kind,
            object.width,
            Some(&previous.logic),
            net_drivers
                .get(&object_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        )
        .map_err(|error| Error::Resolve(error.to_string()))?;
        let next = ObjectValue::from_logic(resolved);
        if frame[object_id] != next {
            frame[object_id] = next;
            changed = true;
        }
    }

    Ok(changed)
}
