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
}
