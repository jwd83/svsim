//! Shared HIR expression evaluation: the `Value` runtime type, its semantic
//! combinators, and `eval_expr` — the single evaluator used by the simulation
//! runtime and (with a parameters-only module and empty memories) by
//! compile-time constant evaluation. Keeping one evaluator means constant
//! folding and simulation cannot diverge per-operator.

use std::collections::HashMap;

use crate::bit_value::BitValue;
use crate::diag::{Error, Result};
use crate::hir::{
    BinaryOp, Expr, ModuleSummary, NumericLiteral, PackedRange, ParameterDecl, UnaryOp,
};
use crate::logic_ops::{
    logic_bit_and, logic_bit_not, logic_bit_or, logic_bit_xor, logic_reduce_and, logic_reduce_or,
    logic_reduce_xor, logic_sign_extend, logic_slice, logic_value_from_bit,
};
use crate::logic_value::{LogicBit, LogicBits, LogicValue};
use crate::width::{expr_width, minimum_width};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Value {
    pub(crate) logic: LogicValue,
    pub(crate) width: usize,
    pub(crate) signed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicTruth {
    False,
    True,
    Unknown,
}

impl Value {
    pub(crate) fn new(bits: BitValue, width: usize) -> Self {
        Self::new_with_signed(bits, width, false)
    }

    pub(crate) fn new_with_signed(bits: BitValue, width: usize, signed: bool) -> Self {
        Self::from_logic_with_signed(LogicValue::from_bit_value_with_width(bits, width), signed)
    }

    pub(crate) fn from_logic(logic: LogicValue, width: usize) -> Self {
        Self::from_logic_with_signed(logic.coerced_to(width), false)
    }

    pub(crate) fn from_logic_with_signed(logic: LogicValue, signed: bool) -> Self {
        let width = logic.width().max(1);
        Self {
            logic: logic.coerced_to(width),
            width,
            signed,
        }
    }

    pub(crate) fn coerced_to(&self, width: usize) -> Self {
        let width = width.max(1);
        let logic = if self.signed {
            logic_sign_extend(&self.logic, self.width, width)
        } else {
            self.logic.coerced_to(width)
        };
        Self::from_logic_with_signed(logic, self.signed)
    }

    pub(crate) fn zero(width: usize) -> Self {
        Self::new(BitValue::zero(), width)
    }

    pub(crate) fn unknown(width: usize) -> Self {
        Self::from_logic(
            LogicValue::new(LogicBits::filled(width, LogicBit::X), width),
            width,
        )
    }

    pub(crate) fn logic(&self) -> &LogicValue {
        &self.logic
    }

    pub(crate) fn to_bit_value_checked(&self) -> Option<BitValue> {
        self.logic.to_bit_value_checked()
    }

    /// Two-state, non-negative conversion for host-side indexes and counts.
    pub(crate) fn to_usize_checked(&self) -> Option<usize> {
        let bits = self.to_bit_value_checked()?;
        if self.signed && bits.get_bit(self.width.max(1) - 1) {
            return None;
        }
        bits.to_usize_checked()
    }

    pub(crate) fn truthiness(&self) -> LogicTruth {
        let mut saw_unknown = false;
        for index in 0..self.width {
            match self.logic.bit(index) {
                LogicBit::One => return LogicTruth::True,
                LogicBit::Zero => {}
                LogicBit::X | LogicBit::Z => saw_unknown = true,
            }
        }
        if saw_unknown {
            LogicTruth::Unknown
        } else {
            LogicTruth::False
        }
    }

    pub(crate) fn truthy(&self) -> bool {
        matches!(self.truthiness(), LogicTruth::True)
    }
}

pub(crate) fn logic_value_from_ordering(ordering: Option<bool>) -> Value {
    match ordering {
        Some(true) => Value::from_logic(logic_value_from_bit(LogicBit::One), 1),
        Some(false) => Value::from_logic(logic_value_from_bit(LogicBit::Zero), 1),
        None => Value::from_logic(logic_value_from_bit(LogicBit::X), 1),
    }
}

pub(crate) fn logic_bitwise_binary(
    left: &Value,
    right: &Value,
    op: fn(LogicBit, LogicBit) -> LogicBit,
) -> Value {
    let width = left.width.max(right.width);
    let left = left.coerced_to(width);
    let right = right.coerced_to(width);
    let mut bits = LogicBits::zero();
    for index in 0..width {
        bits.set_bit(index, op(left.logic.bit(index), right.logic.bit(index)));
    }
    Value::from_logic_with_signed(LogicValue::new(bits, width), left.signed && right.signed)
}

pub(crate) fn logical_and(left: &Value, right: &Value) -> LogicBit {
    match (left.truthiness(), right.truthiness()) {
        (LogicTruth::False, _) | (_, LogicTruth::False) => LogicBit::Zero,
        (LogicTruth::True, LogicTruth::True) => LogicBit::One,
        _ => LogicBit::X,
    }
}

pub(crate) fn logical_or(left: &Value, right: &Value) -> LogicBit {
    match (left.truthiness(), right.truthiness()) {
        (LogicTruth::True, _) | (_, LogicTruth::True) => LogicBit::One,
        (LogicTruth::False, LogicTruth::False) => LogicBit::Zero,
        _ => LogicBit::X,
    }
}

pub(crate) fn values_logical_equal(left: &Value, right: &Value) -> LogicBit {
    let width = left.width.max(right.width);
    let left = left.coerced_to(width);
    let right = right.coerced_to(width);
    let mut saw_unknown = false;

    for index in 0..width {
        match (left.logic.bit(index), right.logic.bit(index)) {
            (LogicBit::Zero, LogicBit::One) | (LogicBit::One, LogicBit::Zero) => {
                return LogicBit::Zero;
            }
            (LogicBit::Zero, LogicBit::Zero) | (LogicBit::One, LogicBit::One) => {}
            _ => saw_unknown = true,
        }
    }

    if saw_unknown {
        LogicBit::X
    } else {
        LogicBit::One
    }
}

pub(crate) fn values_case_equal(left: &Value, right: &Value) -> bool {
    let width = left.width.max(right.width);
    let left = left.coerced_to(width);
    let right = right.coerced_to(width);
    (0..width).all(|index| left.logic.bit(index) == right.logic.bit(index))
}

pub(crate) fn compare_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    let left_bits = left.to_bit_value_checked()?;
    let right_bits = right.to_bit_value_checked()?;
    if left.signed && right.signed {
        Some(compare_signed_bits(&left_bits, &right_bits, left.width))
    } else {
        Some(left_bits.cmp_unsigned(&right_bits))
    }
}

pub(crate) fn logic_shift_left_value(left: &Value, right: &Value) -> Value {
    let Some(shift_bits) = right.to_bit_value_checked() else {
        return Value::unknown(left.width);
    };
    let shift = shift_bits.to_usize_checked().unwrap_or(left.width);
    let mut bits = LogicBits::zero();
    if shift < left.width {
        for index in shift..left.width {
            bits.set_bit(index, left.logic.bit(index - shift));
        }
    }
    Value::from_logic_with_signed(LogicValue::new(bits, left.width), left.signed)
}

pub(crate) fn logic_shift_right_value(left: &Value, right: &Value) -> Value {
    let Some(shift_bits) = right.to_bit_value_checked() else {
        return Value::unknown(left.width);
    };
    let shift = shift_bits.to_usize_checked().unwrap_or(left.width);
    let mut bits = LogicBits::zero();
    if shift < left.width {
        for index in 0..(left.width - shift) {
            bits.set_bit(index, left.logic.bit(index + shift));
        }
    }
    Value::from_logic_with_signed(LogicValue::new(bits, left.width), left.signed)
}

pub(crate) fn logic_arithmetic_shift_right_value(left: &Value, right: &Value) -> Value {
    let Some(shift_bits) = right.to_bit_value_checked() else {
        return Value::unknown(left.width);
    };
    let shift = shift_bits.to_usize_checked().unwrap_or(left.width);
    let fill = left.logic.bit(left.width.saturating_sub(1));
    let mut bits = LogicBits::zero();
    for index in 0..left.width {
        let bit = if index + shift < left.width {
            left.logic.bit(index + shift)
        } else {
            fill
        };
        bits.set_bit(index, bit);
    }
    Value::from_logic_with_signed(LogicValue::new(bits, left.width), left.signed)
}

pub(crate) fn logic_ternary_merge(when_true: &Value, when_false: &Value, width: usize) -> Value {
    let when_true = when_true.coerced_to(width);
    let when_false = when_false.coerced_to(width);
    let mut bits = LogicBits::zero();
    for index in 0..width {
        let bit = if when_true.logic.bit(index) == when_false.logic.bit(index) {
            when_true.logic.bit(index)
        } else {
            LogicBit::X
        };
        bits.set_bit(index, bit);
    }
    Value::from_logic_with_signed(
        LogicValue::new(bits, width),
        when_true.signed && when_false.signed,
    )
}

pub(crate) fn value_from_literal(literal: &NumericLiteral) -> Value {
    let width = literal
        .width
        .unwrap_or_else(|| minimum_width(&literal.bits));
    Value::from_logic(LogicValue::new(literal.bits.clone(), width), width)
}

pub(crate) fn concat_values(parts: &[Value]) -> Result<Value> {
    let mut total_width = 0usize;
    for part in parts {
        total_width = total_width
            .checked_add(part.width)
            .ok_or_else(|| Error::Unsupported("concatenation width exceeds host limits".into()))?;
    }

    let mut bits = LogicBits::zero();
    let mut shift = total_width;
    for part in parts {
        shift -= part.width;
        for offset in 0..part.width {
            bits.set_bit(shift + offset, part.logic.bit(offset));
        }
    }
    Ok(Value::from_logic(
        LogicValue::new(bits, total_width),
        total_width,
    ))
}

pub(crate) fn compare_signed_bits(
    left: &BitValue,
    right: &BitValue,
    width: usize,
) -> std::cmp::Ordering {
    let width = width.max(1);
    let left = left.truncate(width);
    let right = right.truncate(width);
    match left.get_bit(width - 1).cmp(&right.get_bit(width - 1)) {
        std::cmp::Ordering::Less => std::cmp::Ordering::Greater,
        std::cmp::Ordering::Greater => std::cmp::Ordering::Less,
        std::cmp::Ordering::Equal => left.cmp_unsigned(&right),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryState {
    pub(crate) index_range: PackedRange,
    pub(crate) words: Vec<Value>,
}

impl MemoryState {
    pub(crate) fn read(&self, index: usize, memory_name: &str) -> Result<Value> {
        let offset = self.index_range.index_offset(index).ok_or_else(|| {
            Error::Resolve(format!(
                "memory index [{}] is out of range for '{}'",
                index, memory_name
            ))
        })?;
        Ok(self.words[offset].clone())
    }

    pub(crate) fn write(&mut self, index: usize, value: Value, memory_name: &str) -> Result<bool> {
        let offset = self.index_range.index_offset(index).ok_or_else(|| {
            Error::Resolve(format!(
                "memory index [{}] is out of range for '{}'",
                index, memory_name
            ))
        })?;
        let current = self
            .words
            .get_mut(offset)
            .expect("memory offset is guaranteed to be in range");
        let coerced = value.coerced_to(current.width);
        let next = Value::from_logic(coerced.logic, current.width);
        let changed = *current != next;
        *current = next;
        Ok(changed)
    }
}

/// Read seam for expression evaluation. The simulation runtime reads signal
/// values straight from its indexed frame through this trait, while constant
/// contexts (validation, frontend folding, elaboration) keep a plain map.
pub(crate) trait ValueReader {
    fn read_value(&self, name: &str) -> Option<Value>;
}

impl<S: std::hash::BuildHasher> ValueReader for HashMap<String, Value, S> {
    fn read_value(&self, name: &str) -> Option<Value> {
        self.get(name).cloned()
    }
}

pub(crate) fn eval_expr<S: std::hash::BuildHasher>(
    expr: &Expr,
    module: &ModuleSummary,
    values: &impl ValueReader,
    memories: &HashMap<String, MemoryState, S>,
) -> Result<Value> {
    match expr {
        Expr::Ident(name) => values.read_value(name).ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                name, module.name
            ))
        }),
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
                .ok_or_else(|| {
                    Error::Resolve(format!("memory index for '{}' exceeds host limits", memory))
                })?;
            let memory_state = memories.get(memory).ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            })?;
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
                    logic_value_from_bit(logic_reduce_or(value.logic())),
                    1,
                )),
                UnaryOp::ReductionAnd => Ok(Value::from_logic(
                    logic_value_from_bit(logic_reduce_and(value.logic())),
                    1,
                )),
                UnaryOp::ReductionNand => Ok(Value::from_logic(
                    logic_value_from_bit(logic_bit_not(logic_reduce_and(value.logic()))),
                    1,
                )),
                UnaryOp::ReductionXor => Ok(Value::from_logic(
                    logic_value_from_bit(logic_reduce_xor(value.logic())),
                    1,
                )),
                UnaryOp::Signed => Ok(Value::from_logic_with_signed(value.logic, true)),
                UnaryOp::Unsigned => Ok(Value::from_logic_with_signed(value.logic, false)),
            }
        }
        Expr::Binary { left, op, right } => {
            // Logical operators short-circuit: when the left operand decides
            // the result, the right operand is not evaluated. This lets
            // constant folding prune guarded expressions that would not
            // evaluate (the historical frontend behavior), and is value-
            // identical for runtime operands.
            if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                let left_value = eval_expr(left, module, values, memories)?;
                let bit = match (op, left_value.truthiness()) {
                    (BinaryOp::LogicalAnd, LogicTruth::False) => LogicBit::Zero,
                    (BinaryOp::LogicalOr, LogicTruth::True) => LogicBit::One,
                    (BinaryOp::LogicalAnd, _) => {
                        let right_value = eval_expr(right, module, values, memories)?;
                        logical_and(&left_value, &right_value)
                    }
                    (_, _) => {
                        let right_value = eval_expr(right, module, values, memories)?;
                        logical_or(&left_value, &right_value)
                    }
                };
                return Ok(Value::from_logic(logic_value_from_bit(bit), 1));
            }
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

/// Resolves parameter default values in declaration order, coercing each to
/// its declared width — the same rule the runtime uses when no instance
/// overrides are in play.
pub(crate) fn resolve_parameter_defaults(
    params: &[ParameterDecl],
    module: &ModuleSummary,
) -> Result<HashMap<String, Value>> {
    let memories = HashMap::new();
    let mut values = HashMap::new();
    for param in params {
        let value = eval_expr(&param.default_value, module, &values, &memories)?;
        values.insert(param.name.clone(), value.coerced_to(param.width()));
    }
    Ok(values)
}
