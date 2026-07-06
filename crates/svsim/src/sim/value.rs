//! Four-state runtime values and the primitive logic operations on them.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Value {
    pub(super) logic: LogicValue,
    pub(super) width: usize,
    pub(super) signed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogicTruth {
    False,
    True,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectValue {
    pub(super) logic: LogicValue,
}

impl Value {
    pub(super) fn new(bits: BitValue, width: usize) -> Self {
        Self::new_with_signed(bits, width, false)
    }

    pub(super) fn new_with_signed(bits: BitValue, width: usize, signed: bool) -> Self {
        Self::from_logic_with_signed(LogicValue::from_bit_value_with_width(bits, width), signed)
    }

    pub(super) fn from_logic(logic: LogicValue, width: usize) -> Self {
        Self::from_logic_with_signed(logic.coerced_to(width), false)
    }

    pub(super) fn from_logic_with_signed(logic: LogicValue, signed: bool) -> Self {
        let width = logic.width().max(1);
        Self {
            logic: logic.coerced_to(width),
            width,
            signed,
        }
    }

    pub(super) fn coerced_to(&self, width: usize) -> Self {
        let width = width.max(1);
        let logic = if self.signed {
            logic_sign_extend(&self.logic, self.width, width)
        } else {
            self.logic.coerced_to(width)
        };
        Self::from_logic_with_signed(logic, self.signed)
    }

    pub(super) fn zero(width: usize) -> Self {
        Self::new(BitValue::zero(), width)
    }

    pub(super) fn unknown(width: usize) -> Self {
        Self::from_logic(
            LogicValue::new(LogicBits::filled(width, LogicBit::X), width),
            width,
        )
    }

    pub(super) fn logic(&self) -> &LogicValue {
        &self.logic
    }

    pub(super) fn to_bit_value_checked(&self) -> Option<BitValue> {
        self.logic.to_bit_value_checked()
    }

    pub(super) fn truthiness(&self) -> LogicTruth {
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

    pub(super) fn truthy(&self) -> bool {
        matches!(self.truthiness(), LogicTruth::True)
    }
}

impl ObjectValue {
    pub(super) fn zero(width: usize) -> Self {
        Self {
            logic: LogicValue::zero(width),
        }
    }

    pub(super) fn from_logic(logic: LogicValue) -> Self {
        Self { logic }
    }
}

pub(super) fn logic_inputs_from_public_bits(
    inputs: &BTreeMap<String, BitValue>,
) -> BTreeMap<String, LogicValue> {
    inputs
        .iter()
        .map(|(name, value)| (name.clone(), LogicValue::from(value.clone())))
        .collect()
}

pub(super) fn logic_outputs_to_public_bits(
    logic_outputs: BTreeMap<String, LogicValue>,
) -> Result<BTreeMap<String, BitValue>> {
    let mut outputs = BTreeMap::new();

    for (name, logic) in logic_outputs {
        outputs.insert(
            name.clone(),
            logic_to_public_bit_value(&logic, format!("output '{}'", name))?,
        );
    }

    Ok(outputs)
}

pub(super) fn logic_to_public_bit_value(logic: &LogicValue, context: String) -> Result<BitValue> {
    logic
        .to_bit_value_checked()
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{} resolved to four-state value '{}' and cannot be represented through the explicit 2-state wrapper",
                context, logic
            ))
        })
        .map(|bits| bits.truncate(logic.width()))
}

pub(super) fn logic_sign_extend(
    logic: &LogicValue,
    from_width: usize,
    to_width: usize,
) -> LogicValue {
    let to_width = to_width.max(1);
    let from_width = from_width.max(1);
    let mut bits = LogicBits::zero();
    let sign = logic.bit(from_width - 1);
    for index in 0..to_width {
        let bit = if index < from_width {
            logic.bit(index)
        } else {
            sign
        };
        bits.set_bit(index, bit);
    }
    LogicValue::new(bits, to_width)
}

pub(super) fn logic_slice(value: &LogicValue, low: usize, width: usize) -> LogicValue {
    let width = width.max(1);
    let mut bits = LogicBits::zero();
    for offset in 0..width {
        bits.set_bit(offset, value.bit(low + offset));
    }
    LogicValue::new(bits, width)
}

pub(super) fn logic_replace_slice(
    base: &LogicValue,
    low: usize,
    width: usize,
    replacement: &LogicValue,
) -> LogicValue {
    let mut bits = LogicBits::zero();
    let replacement = replacement.coerced_to(width);
    for index in 0..base.width() {
        let bit = if (low..low + width).contains(&index) {
            replacement.bit(index - low)
        } else {
            base.bit(index)
        };
        bits.set_bit(index, bit);
    }
    LogicValue::new(bits, base.width())
}

pub(super) fn logic_value_from_ordering(ordering: Option<bool>) -> Value {
    match ordering {
        Some(true) => Value::from_logic(logic_value_from_bit(LogicBit::One), 1),
        Some(false) => Value::from_logic(logic_value_from_bit(LogicBit::Zero), 1),
        None => Value::from_logic(logic_value_from_bit(LogicBit::X), 1),
    }
}

pub(super) fn logic_reduce_or(value: &Value) -> LogicBit {
    match value.truthiness() {
        LogicTruth::False => LogicBit::Zero,
        LogicTruth::True => LogicBit::One,
        LogicTruth::Unknown => LogicBit::X,
    }
}

pub(super) fn logic_reduce_and(value: &Value) -> LogicBit {
    let mut saw_unknown = false;
    for index in 0..value.width {
        match value.logic.bit(index) {
            LogicBit::Zero => return LogicBit::Zero,
            LogicBit::One => {}
            LogicBit::X | LogicBit::Z => saw_unknown = true,
        }
    }
    if saw_unknown {
        LogicBit::X
    } else {
        LogicBit::One
    }
}

pub(super) fn logic_reduce_xor(value: &Value) -> LogicBit {
    let mut parity = false;
    for index in 0..value.width {
        match value.logic.bit(index) {
            LogicBit::Zero => {}
            LogicBit::One => parity = !parity,
            LogicBit::X | LogicBit::Z => return LogicBit::X,
        }
    }
    if parity {
        LogicBit::One
    } else {
        LogicBit::Zero
    }
}

pub(super) fn logic_bitwise_binary(
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

pub(super) fn logical_and(left: &Value, right: &Value) -> LogicBit {
    match (left.truthiness(), right.truthiness()) {
        (LogicTruth::False, _) | (_, LogicTruth::False) => LogicBit::Zero,
        (LogicTruth::True, LogicTruth::True) => LogicBit::One,
        _ => LogicBit::X,
    }
}

pub(super) fn logical_or(left: &Value, right: &Value) -> LogicBit {
    match (left.truthiness(), right.truthiness()) {
        (LogicTruth::True, _) | (_, LogicTruth::True) => LogicBit::One,
        (LogicTruth::False, LogicTruth::False) => LogicBit::Zero,
        _ => LogicBit::X,
    }
}

pub(super) fn values_logical_equal(left: &Value, right: &Value) -> LogicBit {
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

pub(super) fn values_case_equal(left: &Value, right: &Value) -> bool {
    let width = left.width.max(right.width);
    let left = left.coerced_to(width);
    let right = right.coerced_to(width);
    (0..width).all(|index| left.logic.bit(index) == right.logic.bit(index))
}

pub(super) fn compare_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    let left_bits = left.to_bit_value_checked()?;
    let right_bits = right.to_bit_value_checked()?;
    if left.signed && right.signed {
        Some(compare_signed_bits(&left_bits, &right_bits, left.width))
    } else {
        Some(left_bits.cmp_unsigned(&right_bits))
    }
}

pub(super) fn logic_shift_left_value(left: &Value, right: &Value) -> Value {
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

pub(super) fn logic_shift_right_value(left: &Value, right: &Value) -> Value {
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

pub(super) fn logic_arithmetic_shift_right_value(left: &Value, right: &Value) -> Value {
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

pub(super) fn logic_ternary_merge(when_true: &Value, when_false: &Value, width: usize) -> Value {
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

pub(super) fn value_from_literal(literal: &NumericLiteral) -> Value {
    let width = literal
        .width
        .unwrap_or_else(|| minimum_width(&literal.bits));
    Value::from_logic(LogicValue::new(literal.bits.clone(), width), width)
}

pub(super) fn concat_values(parts: &[Value]) -> Result<Value> {
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

pub(super) fn compare_signed_bits(
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
