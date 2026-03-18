use crate::bit_value::BitValue;
use crate::diag::{Error, Result};
use crate::hir::{BinaryOp, Expr, ModuleSummary};

pub(crate) fn expr_width(expr: &Expr, module: &ModuleSummary) -> Result<usize> {
    match expr {
        Expr::Ident(name) => module.signal_width(name).ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                name, module.name
            ))
        }),
        Expr::Literal(literal) => Ok(literal
            .width
            .unwrap_or_else(|| minimum_width(&literal.bits))),
        Expr::Concat(exprs) => {
            let mut width = 0usize;
            for expr in exprs {
                width = width.saturating_add(expr_width(expr, module)?);
            }
            Ok(width)
        }
        Expr::Repeat { count, expr } => Ok(expr_width(expr, module)?.saturating_mul(*count)),
        Expr::MemoryRead { memory, .. } => module
            .memory_decl(memory)
            .map(|memory| memory.element_width())
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            }),
        Expr::BitSelect { expr, index } => {
            let width = expr_width(expr, module)?;
            if *index >= width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for width {}",
                    index, width
                )));
            }
            Ok(1)
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let width = expr_width(expr, module)?;
            let high = (*msb).max(*lsb);
            if high >= width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for width {}",
                    msb, lsb, width
                )));
            }
            Ok(high - (*msb).min(*lsb) + 1)
        }
        Expr::Unary { expr, .. } => expr_width(expr, module),
        Expr::Binary { left, op, right } => {
            let left_width = expr_width(left, module)?;
            let right_width = expr_width(right, module)?;
            Ok(match op {
                BinaryOp::LogicalAnd
                | BinaryOp::LogicalOr
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::LtEq
                | BinaryOp::Gt
                | BinaryOp::GtEq => 1,
                BinaryOp::ShiftLeft | BinaryOp::ShiftRight => left_width,
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
            expr_width(cond, module)?;
            let when_true_width = expr_width(when_true, module)?;
            let when_false_width = expr_width(when_false, module)?;
            Ok(when_true_width.max(when_false_width))
        }
    }
}

pub(crate) fn minimum_width(bits: &BitValue) -> usize {
    bits.bit_len().max(1)
}

pub(crate) fn mask(width: usize) -> BitValue {
    BitValue::mask(width)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ShiftDirection {
    Left,
    Right,
}

pub(crate) fn shift_bits(
    bits: &BitValue,
    amount_bits: &BitValue,
    width: usize,
    direction: ShiftDirection,
) -> BitValue {
    let normalized = bits.truncate(width);
    let Some(amount) = amount_bits.to_usize_checked() else {
        return BitValue::zero();
    };
    if amount >= width {
        return BitValue::zero();
    }
    match direction {
        ShiftDirection::Left => normalized.shift_left(amount).truncate(width),
        ShiftDirection::Right => normalized.shift_right(amount),
    }
}

pub(crate) fn shift_left_bits(bits: &BitValue, amount_bits: &BitValue, width: usize) -> BitValue {
    shift_bits(bits, amount_bits, width, ShiftDirection::Left)
}

pub(crate) fn shift_right_bits(bits: &BitValue, amount_bits: &BitValue, width: usize) -> BitValue {
    shift_bits(bits, amount_bits, width, ShiftDirection::Right)
}
