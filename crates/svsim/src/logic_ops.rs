//! Primitive four-state logic operations shared by the runtime evaluator and
//! (eventually) constant evaluation. These are the truth tables that define
//! simulator correctness; test them here directly, not through whole-design
//! simulations.

use crate::logic_value::{LogicBit, LogicBits, LogicValue};

/// IEEE 1800 collapses `z` operands to `x` for the ordinary logic operators.
pub(crate) fn normalize_unknown_bit(bit: LogicBit) -> LogicBit {
    match bit {
        LogicBit::Zero => LogicBit::Zero,
        LogicBit::One => LogicBit::One,
        LogicBit::X | LogicBit::Z => LogicBit::X,
    }
}

pub(crate) fn logic_bit_not(bit: LogicBit) -> LogicBit {
    match bit {
        LogicBit::Zero => LogicBit::One,
        LogicBit::One => LogicBit::Zero,
        LogicBit::X | LogicBit::Z => LogicBit::X,
    }
}

pub(crate) fn logic_bit_and(left: LogicBit, right: LogicBit) -> LogicBit {
    match (normalize_unknown_bit(left), normalize_unknown_bit(right)) {
        (LogicBit::Zero, _) | (_, LogicBit::Zero) => LogicBit::Zero,
        (LogicBit::One, LogicBit::One) => LogicBit::One,
        _ => LogicBit::X,
    }
}

pub(crate) fn logic_bit_or(left: LogicBit, right: LogicBit) -> LogicBit {
    match (normalize_unknown_bit(left), normalize_unknown_bit(right)) {
        (LogicBit::One, _) | (_, LogicBit::One) => LogicBit::One,
        (LogicBit::Zero, LogicBit::Zero) => LogicBit::Zero,
        _ => LogicBit::X,
    }
}

pub(crate) fn logic_bit_xor(left: LogicBit, right: LogicBit) -> LogicBit {
    match (normalize_unknown_bit(left), normalize_unknown_bit(right)) {
        (LogicBit::Zero, LogicBit::Zero) | (LogicBit::One, LogicBit::One) => LogicBit::Zero,
        (LogicBit::Zero, LogicBit::One) | (LogicBit::One, LogicBit::Zero) => LogicBit::One,
        _ => LogicBit::X,
    }
}

pub(crate) fn logic_value_from_bit(bit: LogicBit) -> LogicValue {
    let mut bits = LogicBits::zero();
    bits.set_bit(0, bit);
    LogicValue::new(bits, 1)
}

pub(crate) fn logic_sign_extend(
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

pub(crate) fn logic_slice(value: &LogicValue, low: usize, width: usize) -> LogicValue {
    let width = width.max(1);
    let mut bits = LogicBits::zero();
    for offset in 0..width {
        bits.set_bit(offset, value.bit(low + offset));
    }
    LogicValue::new(bits, width)
}

pub(crate) fn logic_replace_slice(
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

pub(crate) fn logic_reduce_or(value: &LogicValue) -> LogicBit {
    let mut saw_unknown = false;
    for index in 0..value.width() {
        match value.bit(index) {
            LogicBit::One => return LogicBit::One,
            LogicBit::Zero => {}
            LogicBit::X | LogicBit::Z => saw_unknown = true,
        }
    }
    if saw_unknown {
        LogicBit::X
    } else {
        LogicBit::Zero
    }
}

pub(crate) fn logic_reduce_and(value: &LogicValue) -> LogicBit {
    let mut saw_unknown = false;
    for index in 0..value.width() {
        match value.bit(index) {
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

pub(crate) fn logic_reduce_xor(value: &LogicValue) -> LogicBit {
    let mut parity = false;
    for index in 0..value.width() {
        match value.bit(index) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use LogicBit::{One, X, Z, Zero};

    const ALL_BITS: [LogicBit; 4] = [Zero, One, X, Z];

    #[test]
    fn normalize_unknown_bit_collapses_z_to_x() {
        assert_eq!(normalize_unknown_bit(Zero), Zero);
        assert_eq!(normalize_unknown_bit(One), One);
        assert_eq!(normalize_unknown_bit(X), X);
        assert_eq!(normalize_unknown_bit(Z), X);
    }

    #[test]
    fn not_truth_table_inverts_known_bits_and_unknowns_stay_x() {
        assert_eq!(logic_bit_not(Zero), One);
        assert_eq!(logic_bit_not(One), Zero);
        assert_eq!(logic_bit_not(X), X);
        assert_eq!(logic_bit_not(Z), X);
    }

    #[test]
    fn and_truth_table_matches_ieee_1800() {
        // Zero dominates; One is identity; anything else is X.
        for bit in ALL_BITS {
            assert_eq!(logic_bit_and(Zero, bit), Zero);
            assert_eq!(logic_bit_and(bit, Zero), Zero);
        }
        assert_eq!(logic_bit_and(One, One), One);
        for unknown in [X, Z] {
            assert_eq!(logic_bit_and(One, unknown), X);
            assert_eq!(logic_bit_and(unknown, One), X);
            for other in [X, Z] {
                assert_eq!(logic_bit_and(unknown, other), X);
            }
        }
    }

    #[test]
    fn or_truth_table_matches_ieee_1800() {
        // One dominates; Zero is identity; anything else is X.
        for bit in ALL_BITS {
            assert_eq!(logic_bit_or(One, bit), One);
            assert_eq!(logic_bit_or(bit, One), One);
        }
        assert_eq!(logic_bit_or(Zero, Zero), Zero);
        for unknown in [X, Z] {
            assert_eq!(logic_bit_or(Zero, unknown), X);
            assert_eq!(logic_bit_or(unknown, Zero), X);
            for other in [X, Z] {
                assert_eq!(logic_bit_or(unknown, other), X);
            }
        }
    }

    #[test]
    fn xor_truth_table_matches_ieee_1800() {
        assert_eq!(logic_bit_xor(Zero, Zero), Zero);
        assert_eq!(logic_bit_xor(One, One), Zero);
        assert_eq!(logic_bit_xor(Zero, One), One);
        assert_eq!(logic_bit_xor(One, Zero), One);
        // Any unknown operand poisons the result.
        for unknown in [X, Z] {
            for bit in ALL_BITS {
                assert_eq!(logic_bit_xor(unknown, bit), X);
                assert_eq!(logic_bit_xor(bit, unknown), X);
            }
        }
    }

    #[test]
    fn logic_value_from_bit_builds_one_bit_values() {
        for bit in ALL_BITS {
            let value = logic_value_from_bit(bit);
            assert_eq!(value.width(), 1);
            assert_eq!(value.bit(0), bit);
        }
    }

    fn logic(text: &str) -> LogicValue {
        LogicValue::from_logic_str(text).expect("parse logic literal")
    }

    #[test]
    fn sign_extend_replicates_the_sign_bit_including_x_and_z() {
        assert_eq!(logic_sign_extend(&logic("0101"), 4, 8), logic("00000101"));
        assert_eq!(logic_sign_extend(&logic("1010"), 4, 8), logic("11111010"));
        assert_eq!(logic_sign_extend(&logic("x010"), 4, 8), logic("xxxxx010"));
        assert_eq!(logic_sign_extend(&logic("z010"), 4, 8), logic("zzzzz010"));
    }

    #[test]
    fn sign_extend_to_narrower_width_keeps_low_bits() {
        assert_eq!(logic_sign_extend(&logic("1100_1010"), 8, 4), logic("1010"));
    }

    #[test]
    fn slice_extracts_bits_preserving_x_and_z() {
        let value = logic("01xz0110");
        assert_eq!(logic_slice(&value, 0, 4), logic("0110"));
        assert_eq!(logic_slice(&value, 4, 4), logic("01xz"));
        assert_eq!(logic_slice(&value, 3, 3), logic("xz0"));
    }

    #[test]
    fn replace_slice_overwrites_only_the_target_range() {
        let base = logic("1111_1111");
        assert_eq!(
            logic_replace_slice(&base, 2, 4, &logic("0xz0")),
            logic("110xz011")
        );
    }

    #[test]
    fn replace_slice_coerces_replacement_to_the_range_width() {
        let base = logic("0000_0000");
        // Wider replacement is truncated to the 2-bit range.
        assert_eq!(
            logic_replace_slice(&base, 0, 2, &logic("1111")),
            logic("00000011")
        );
    }

    #[test]
    fn reduce_or_finds_any_one_and_reports_unknown_otherwise() {
        assert_eq!(logic_reduce_or(&logic("0000")), Zero);
        assert_eq!(logic_reduce_or(&logic("0010")), One);
        // A known One dominates unknowns.
        assert_eq!(logic_reduce_or(&logic("x1z0")), One);
        assert_eq!(logic_reduce_or(&logic("x000")), X);
        assert_eq!(logic_reduce_or(&logic("z000")), X);
    }

    #[test]
    fn reduce_and_finds_any_zero_and_reports_unknown_otherwise() {
        assert_eq!(logic_reduce_and(&logic("1111")), One);
        assert_eq!(logic_reduce_and(&logic("1101")), Zero);
        // A known Zero dominates unknowns.
        assert_eq!(logic_reduce_and(&logic("x0z1")), Zero);
        assert_eq!(logic_reduce_and(&logic("x111")), X);
        assert_eq!(logic_reduce_and(&logic("z111")), X);
    }

    #[test]
    fn reduce_xor_computes_parity_and_any_unknown_poisons_it() {
        assert_eq!(logic_reduce_xor(&logic("0000")), Zero);
        assert_eq!(logic_reduce_xor(&logic("0110")), Zero);
        assert_eq!(logic_reduce_xor(&logic("0010")), One);
        assert_eq!(logic_reduce_xor(&logic("1110")), One);
        assert_eq!(logic_reduce_xor(&logic("1x11")), X);
        assert_eq!(logic_reduce_xor(&logic("1z11")), X);
    }
}
