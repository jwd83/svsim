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
        Expr::Literal(literal) => Ok(literal.width.unwrap_or_else(|| minimum_width(literal.bits))),
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
            expr_width(cond, module)?;
            let when_true_width = expr_width(when_true, module)?;
            let when_false_width = expr_width(when_false, module)?;
            Ok(when_true_width.max(when_false_width))
        }
    }
}

pub(crate) fn minimum_width(bits: u64) -> usize {
    if bits == 0 {
        1
    } else {
        (u64::BITS - bits.leading_zeros()) as usize
    }
}
