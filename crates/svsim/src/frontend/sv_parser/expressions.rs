//! Expression, lvalue, select, and operator lowering into HIR `Expr` and
//! `LValue`, including the constant-expression variants used by
//! declaration ranges and parameter defaults. Selects and ranges lower to
//! concrete indices via the `const_eval` funnels.

use super::*;

pub(super) fn lower_constant_param_expression(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::ConstantParamExpression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        sv_parser::ConstantParamExpression::ConstantMintypmaxExpression(cmtm) => {
            lower_constant_mintypmax_to_expr(syntax_tree, cmtm, module, path)
        }
        _ => Err(unsupported(
            "parameter default expression is outside the supported subset",
            None,
        )),
    }
}

pub(super) fn lower_constant_mintypmax_to_expr(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::ConstantMintypmaxExpression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        sv_parser::ConstantMintypmaxExpression::Unary(ce) => {
            lower_constant_expression_to_expr(syntax_tree, ce, module, path)
        }
        sv_parser::ConstantMintypmaxExpression::Ternary(t) => Ok(Expr::Ternary {
            cond: Box::new(lower_constant_expression_to_expr(
                syntax_tree,
                &t.nodes.0,
                module,
                path,
            )?),
            when_true: Box::new(lower_constant_expression_to_expr(
                syntax_tree,
                &t.nodes.2,
                module,
                path,
            )?),
            when_false: Box::new(lower_constant_expression_to_expr(
                syntax_tree,
                &t.nodes.4,
                module,
                path,
            )?),
        }),
    }
}

pub(super) fn lower_constant_expression_to_expr(
    syntax_tree: &SyntaxTree,
    expr: &ConstantExpression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        ConstantExpression::ConstantPrimary(primary) => {
            lower_constant_primary_to_expr(syntax_tree, primary, module, path)
        }
        ConstantExpression::Unary(u) => {
            let op = lower_unary_operator(syntax_tree, &u.nodes.0)?;
            let operand = lower_constant_primary_to_expr(syntax_tree, &u.nodes.2, module, path)?;
            Ok(Expr::Unary {
                op,
                expr: Box::new(operand),
            })
        }
        ConstantExpression::Binary(b) => {
            let left = lower_constant_expression_to_expr(syntax_tree, &b.nodes.0, module, path)?;
            let op = lower_binary_operator(syntax_tree, &b.nodes.1)?;
            let right = lower_constant_expression_to_expr(syntax_tree, &b.nodes.3, module, path)?;
            Ok(Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        }
        ConstantExpression::Ternary(t) => Ok(Expr::Ternary {
            cond: Box::new(lower_constant_expression_to_expr(
                syntax_tree,
                &t.nodes.0,
                module,
                path,
            )?),
            when_true: Box::new(lower_constant_expression_to_expr(
                syntax_tree,
                &t.nodes.3,
                module,
                path,
            )?),
            when_false: Box::new(lower_constant_expression_to_expr(
                syntax_tree,
                &t.nodes.5,
                module,
                path,
            )?),
        }),
    }
}

pub(super) fn lower_constant_primary_to_expr(
    syntax_tree: &SyntaxTree,
    primary: &sv_parser::ConstantPrimary,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match primary {
        sv_parser::ConstantPrimary::PrimaryLiteral(lit) => lower_literal(syntax_tree, lit),
        sv_parser::ConstantPrimary::PsParameter(ps) => {
            let (name, _) = identifier_name_from_node(syntax_tree, RefNode::from(&ps.nodes.0))
                .ok_or_else(|| unsupported("failed to determine parameter reference name", None))?;
            Ok(Expr::Ident(name))
        }
        sv_parser::ConstantPrimary::MintypmaxExpression(expr) => {
            lower_constant_mintypmax_to_expr(syntax_tree, &expr.nodes.0.nodes.1, module, path)
        }
        sv_parser::ConstantPrimary::Concatenation(concat) => {
            let mut exprs = Vec::new();
            for expr in concat.nodes.0.nodes.0.nodes.1.contents() {
                exprs.push(lower_constant_expression_to_expr(
                    syntax_tree,
                    expr,
                    module,
                    path,
                )?);
            }
            Ok(Expr::Concat(exprs))
        }
        sv_parser::ConstantPrimary::MultipleConcatenation(concat) => {
            let inner = &concat.nodes.0.nodes.0;
            let count_expr =
                lower_constant_expression_to_expr(syntax_tree, &inner.nodes.1.0, module, path)?;
            let Expr::Literal(count_lit) = &count_expr else {
                return Err(unsupported(
                    "replication count must be a literal in parameter expressions",
                    None,
                ));
            };
            let count = count_lit
                .bits
                .to_bit_value_checked()
                .and_then(|bits| bits.to_usize_checked())
                .ok_or_else(|| {
                    unsupported(
                        "replication count must be a two-state literal within host limits",
                        None,
                    )
                })?;
            let mut exprs = Vec::new();
            for expr in inner.nodes.1.1.nodes.0.nodes.1.contents() {
                exprs.push(lower_constant_expression_to_expr(
                    syntax_tree,
                    expr,
                    module,
                    path,
                )?);
            }
            Ok(Expr::Repeat {
                count,
                expr: Box::new(Expr::Concat(exprs)),
            })
        }
        sv_parser::ConstantPrimary::ConstantFunctionCall(call) => {
            lower_function_subroutine_call(syntax_tree, &call.nodes.0, module, path).or_else(|_| {
                // sv-parser often parses bare identifier references (like parameter names)
                // as ConstantFunctionCall with no arguments. Extract the identifier.
                let (name, _) =
                    identifier_name_from_node(syntax_tree, RefNode::from(call.as_ref()))
                        .ok_or_else(|| {
                            unsupported("constant function calls are not supported yet", None)
                        })?;
                Ok(Expr::Ident(name))
            })
        }
        _ => Err(unsupported(
            "constant primary expression is outside the supported subset",
            None,
        )),
    }
}

pub(super) fn lower_expression(
    syntax_tree: &SyntaxTree,
    expr: &Expression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        Expression::Primary(primary) => lower_primary(syntax_tree, primary, module, path),
        Expression::Unary(expr) => {
            let op = lower_unary_operator(syntax_tree, &expr.nodes.0)?;
            Ok(Expr::Unary {
                op,
                expr: Box::new(lower_primary(syntax_tree, &expr.nodes.2, module, path)?),
            })
        }
        Expression::Binary(expr) => {
            let left = lower_expression(syntax_tree, &expr.nodes.0, module, path)?;
            if let Expression::ConditionalExpression(rhs) = &expr.nodes.3 {
                return Ok(Expr::Ternary {
                    cond: Box::new(Expr::Binary {
                        left: Box::new(left),
                        op: lower_binary_operator(syntax_tree, &expr.nodes.1)?,
                        right: Box::new(lower_cond_predicate(
                            syntax_tree,
                            &rhs.nodes.0,
                            module,
                            path,
                        )?),
                    }),
                    when_true: Box::new(lower_expression(syntax_tree, &rhs.nodes.3, module, path)?),
                    when_false: Box::new(lower_expression(
                        syntax_tree,
                        &rhs.nodes.5,
                        module,
                        path,
                    )?),
                });
            }
            let op = lower_binary_operator(syntax_tree, &expr.nodes.1)?;
            let right = lower_expression(syntax_tree, &expr.nodes.3, module, path)?;
            Ok(rebalance_logical_rhs_binary(left, op, right))
        }
        Expression::ConditionalExpression(expr) => {
            let cond = lower_cond_predicate(syntax_tree, &expr.nodes.0, module, path)?;
            Ok(Expr::Ternary {
                cond: Box::new(cond),
                when_true: Box::new(lower_expression(syntax_tree, &expr.nodes.3, module, path)?),
                when_false: Box::new(lower_expression(syntax_tree, &expr.nodes.5, module, path)?),
            })
        }
        _ => Err(unsupported(
            "expression is outside the current executable subset",
            None,
        )),
    }
}

pub(super) fn rebalance_logical_rhs_binary(left: Expr, op: BinaryOp, right: Expr) -> Expr {
    let should_rebalance = matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
    );
    if !should_rebalance {
        return Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        };
    }

    match right {
        Expr::Binary {
            left: right_left,
            op: right_op,
            right: right_right,
        } if matches!(right_op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) => Expr::Binary {
            left: Box::new(Expr::Binary {
                left: Box::new(left),
                op,
                right: right_left,
            }),
            op: right_op,
            right: right_right,
        },
        other => Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(other),
        },
    }
}

pub(super) fn lower_primary(
    syntax_tree: &SyntaxTree,
    primary: &Primary,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match primary {
        Primary::PrimaryLiteral(literal) => lower_literal(syntax_tree, literal),
        Primary::Hierarchical(primary) => {
            let (name, _) = lower_hierarchical_identifier(
                syntax_tree,
                &primary.nodes.1,
                "hierarchical expressions",
            )?;
            lower_expr_select(
                syntax_tree,
                Expr::Ident(name),
                &primary.nodes.2,
                module,
                path,
            )
        }
        Primary::Concatenation(concat) => {
            if concat.nodes.1.is_some() {
                return Err(unsupported(
                    "concatenation primaries with range indexing are not supported yet",
                    None,
                ));
            }
            lower_concatenation(syntax_tree, &concat.nodes.0, module, path)
        }
        Primary::MultipleConcatenation(concat) => {
            if concat.nodes.1.is_some() {
                return Err(unsupported(
                    "replication primaries with range indexing are not supported yet",
                    None,
                ));
            }
            lower_multiple_concatenation(syntax_tree, &concat.nodes.0, module, path)
        }
        Primary::MintypmaxExpression(expr) => {
            lower_mintypmax_expression(syntax_tree, &expr.nodes.0.nodes.1, module, path)
        }
        Primary::FunctionSubroutineCall(call) => {
            lower_function_subroutine_call(syntax_tree, call, module, path)
        }
        _ => Err(unsupported("primary expression is not supported yet", None)),
    }
}

pub(super) fn lower_function_subroutine_call(
    syntax_tree: &SyntaxTree,
    call: &FunctionSubroutineCall,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    let sv_parser::SubroutineCall::SystemTfCall(call) = &call.nodes.0 else {
        return Err(unsupported("primary expression is not supported yet", None));
    };
    let sv_parser::SystemTfCall::ArgExpression(call) = call.as_ref() else {
        return Err(unsupported("primary expression is not supported yet", None));
    };

    let name = syntax_tree
        .get_str(&call.nodes.0.nodes.0)
        .ok_or_else(|| unsupported("failed to read system function name", None))?;
    let op = match name.as_ref() {
        "$signed" => UnaryOp::Signed,
        "$unsigned" => UnaryOp::Unsigned,
        _ => return Err(unsupported("primary expression is not supported yet", None)),
    };

    if call.nodes.1.nodes.1.1.is_some() {
        return Err(unsupported(
            format!("`{name}` clocking event arguments are not supported"),
            None,
        ));
    }

    let args = call.nodes.1.nodes.1.0.contents();
    let [Some(arg)] = args.as_slice() else {
        return Err(unsupported(
            format!("`{name}` expects exactly one expression argument"),
            None,
        ));
    };

    Ok(Expr::Unary {
        op,
        expr: Box::new(lower_expression(syntax_tree, arg, module, path)?),
    })
}

pub(super) fn lower_concatenation(
    syntax_tree: &SyntaxTree,
    concat: &sv_parser::Concatenation,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    let mut exprs = Vec::new();
    for expr in concat.nodes.0.nodes.1.contents() {
        exprs.push(lower_expression(syntax_tree, expr, module, path)?);
    }
    Ok(Expr::Concat(exprs))
}

pub(super) fn lower_multiple_concatenation(
    syntax_tree: &SyntaxTree,
    concat: &sv_parser::MultipleConcatenation,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    Ok(Expr::Repeat {
        count: lower_usize_expression(
            syntax_tree,
            &concat.nodes.0.nodes.1.0,
            module,
            path,
            "a replication count",
        )?,
        expr: Box::new(lower_concatenation(
            syntax_tree,
            &concat.nodes.0.nodes.1.1,
            module,
            path,
        )?),
    })
}

pub(super) fn lower_mintypmax_expression(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::MintypmaxExpression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        sv_parser::MintypmaxExpression::Expression(expr) => {
            lower_expression(syntax_tree, expr, module, path)
        }
        sv_parser::MintypmaxExpression::Ternary(expr) => Ok(Expr::Ternary {
            cond: Box::new(lower_expression(syntax_tree, &expr.nodes.0, module, path)?),
            when_true: Box::new(lower_expression(syntax_tree, &expr.nodes.2, module, path)?),
            when_false: Box::new(lower_expression(syntax_tree, &expr.nodes.4, module, path)?),
        }),
    }
}

pub(super) fn lower_expr_select(
    syntax_tree: &SyntaxTree,
    base: Expr,
    select: &Select,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    if select.nodes.0.is_some() {
        return Err(unsupported("member selections are not supported yet", None));
    }

    if let Expr::Ident(name) = &base {
        if module.memory_decl(name).is_some() {
            return match select.nodes.1.nodes.0.as_slice() {
                [index] if select.nodes.2.is_none() => Ok(Expr::MemoryRead {
                    memory: name.clone(),
                    index: Box::new(lower_expression(syntax_tree, &index.nodes.1, module, path)?),
                }),
                [] => Ok(base),
                _ => Err(unsupported(
                    "memory reads only support a single element index today",
                    None,
                )),
            };
        }
    }

    let mut expr = base;
    let bit_selects = &select.nodes.1.nodes.0;
    match bit_selects.as_slice() {
        [] => {}
        [index] => {
            expr = Expr::BitSelect {
                expr: Box::new(expr),
                index: lower_usize_expression(
                    syntax_tree,
                    &index.nodes.1,
                    module,
                    path,
                    "a constant bit select",
                )?,
            };
        }
        _ => {
            return Err(unsupported(
                "multidimensional bit selects are not supported yet",
                None,
            ));
        }
    }

    if let Some(range) = select.nodes.2.as_ref() {
        let (msb, lsb) = lower_part_select_range(syntax_tree, &range.nodes.1, module, path)?;
        expr = Expr::PartSelect {
            expr: Box::new(expr),
            msb,
            lsb,
        };
    }

    Ok(expr)
}

pub(super) fn lower_net_lvalue(
    syntax_tree: &SyntaxTree,
    lvalue: &NetLvalue,
    path: &Path,
) -> LowerResult<LValue> {
    match lvalue {
        NetLvalue::Identifier(lvalue) => {
            let (name, _) = lower_net_identifier(syntax_tree, &lvalue.nodes.0, "net lvalues")?;
            lower_constant_select_lvalue(syntax_tree, name, &lvalue.nodes.1, path)
        }
        NetLvalue::Lvalue(lvalue) => {
            let mut items = Vec::new();
            for item in lvalue.nodes.0.nodes.1.contents() {
                items.push(lower_net_lvalue(syntax_tree, item, path)?);
            }
            Ok(LValue::Concat(items))
        }
        _ => Err(unsupported(
            "complex net lvalues are not supported yet",
            None,
        )),
    }
}

pub(super) fn lower_variable_assignment_lvalue(
    syntax_tree: &SyntaxTree,
    assignment: &VariableAssignment,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<LValue> {
    lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)
}

pub(super) fn lower_variable_lvalue(
    syntax_tree: &SyntaxTree,
    lvalue: &VariableLvalue,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<LValue> {
    match lvalue {
        VariableLvalue::Identifier(lvalue) => {
            let (name, _) = lower_hierarchical_identifier(
                syntax_tree,
                &lvalue.nodes.1.nodes.0,
                "variable lvalues",
            )?;
            lower_select_lvalue(syntax_tree, name, &lvalue.nodes.2, module, path)
        }
        VariableLvalue::Lvalue(lvalue) => {
            let mut items = Vec::new();
            for item in lvalue.nodes.0.nodes.1.contents() {
                items.push(lower_variable_lvalue(syntax_tree, item, module, path)?);
            }
            Ok(LValue::Concat(items))
        }
        _ => Err(unsupported(
            "complex variable lvalues are not supported yet",
            None,
        )),
    }
}

pub(super) fn lower_constant_select_lvalue(
    syntax_tree: &SyntaxTree,
    name: String,
    select: &ConstantSelect,
    path: &Path,
) -> LowerResult<LValue> {
    if select.nodes.0.is_some() {
        return Err(unsupported("member selections are not supported yet", None));
    }

    match select.nodes.1.nodes.0.as_slice() {
        [] => {}
        [index] => {
            return Ok(LValue::BitSelect {
                signal: name,
                index: lower_usize_constant_expression(syntax_tree, &index.nodes.1, path)?,
            });
        }
        _ => {
            return Err(unsupported(
                "multidimensional bit selects are not supported yet",
                None,
            ));
        }
    }

    if let Some(range) = select.nodes.2.as_ref() {
        let (msb, lsb) = lower_constant_part_select_range(syntax_tree, &range.nodes.1, path)?;
        Ok(LValue::PartSelect {
            signal: name,
            msb,
            lsb,
        })
    } else {
        Ok(LValue::Signal(name))
    }
}

pub(super) fn lower_select_lvalue(
    syntax_tree: &SyntaxTree,
    name: String,
    select: &Select,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<LValue> {
    if select.nodes.0.is_some() {
        return Err(unsupported("member selections are not supported yet", None));
    }

    if module.memory_decl(&name).is_some() {
        return match select.nodes.1.nodes.0.as_slice() {
            [index] if select.nodes.2.is_none() => Ok(LValue::MemoryElement {
                memory: name,
                index: Box::new(lower_expression(syntax_tree, &index.nodes.1, module, path)?),
            }),
            [] => Err(unsupported(
                "assignments must target a single memory element",
                None,
            )),
            _ => Err(unsupported(
                "memory element assignments only support a single element index today",
                None,
            )),
        };
    }

    match select.nodes.1.nodes.0.as_slice() {
        [] => {}
        [index] => {
            return Ok(LValue::BitSelect {
                signal: name,
                index: lower_usize_expression(
                    syntax_tree,
                    &index.nodes.1,
                    module,
                    path,
                    "a constant bit select",
                )?,
            });
        }
        _ => {
            return Err(unsupported(
                "multidimensional bit selects are not supported yet",
                None,
            ));
        }
    }

    if let Some(range) = select.nodes.2.as_ref() {
        let (msb, lsb) = lower_part_select_range(syntax_tree, &range.nodes.1, module, path)?;
        Ok(LValue::PartSelect {
            signal: name,
            msb,
            lsb,
        })
    } else {
        Ok(LValue::Signal(name))
    }
}

pub(super) fn lower_hierarchical_identifier(
    syntax_tree: &SyntaxTree,
    identifier: &HierarchicalIdentifier,
    context: &str,
) -> LowerResult<(String, Locate)> {
    if identifier.nodes.0.is_some() || !identifier.nodes.1.is_empty() {
        return Err(unsupported(
            format!("{context} must refer to a local identifier"),
            None,
        ));
    }
    identifier_name_from_node(syntax_tree, RefNode::from(&identifier.nodes.2)).ok_or_else(|| {
        unsupported(
            format!("failed to determine identifier for {context}"),
            None,
        )
    })
}

pub(super) fn lower_net_identifier(
    syntax_tree: &SyntaxTree,
    identifier: &PsOrHierarchicalNetIdentifier,
    context: &str,
) -> LowerResult<(String, Locate)> {
    match identifier {
        PsOrHierarchicalNetIdentifier::PackageScope(identifier) => {
            if identifier.nodes.0.is_some() {
                return Err(unsupported(
                    format!("{context} must not use package scopes"),
                    None,
                ));
            }
            identifier_name_from_node(syntax_tree, RefNode::from(&identifier.nodes.1)).ok_or_else(
                || {
                    unsupported(
                        format!("failed to determine identifier for {context}"),
                        None,
                    )
                },
            )
        }
        PsOrHierarchicalNetIdentifier::HierarchicalNetIdentifier(identifier) => {
            lower_hierarchical_identifier(syntax_tree, &identifier.nodes.0, context)
        }
    }
}

pub(super) fn lower_cond_predicate(
    syntax_tree: &SyntaxTree,
    predicate: &CondPredicate,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    let entries = predicate.nodes.0.contents();
    let [entry] = entries.as_slice() else {
        return Err(unsupported(
            "conditional expressions with multiple predicates are not supported yet",
            None,
        ));
    };
    match entry {
        sv_parser::ExpressionOrCondPattern::Expression(expr) => {
            lower_expression(syntax_tree, expr, module, path)
        }
        _ => Err(unsupported(
            "conditional pattern expressions are not supported yet",
            None,
        )),
    }
}

pub(super) fn lower_unary_operator(
    syntax_tree: &SyntaxTree,
    operator: &UnaryOperator,
) -> LowerResult<UnaryOp> {
    match symbol_text(syntax_tree, &operator.nodes.0)?.as_str() {
        "~" => Ok(UnaryOp::BitNot),
        "-" => Ok(UnaryOp::Negate),
        "!" => Ok(UnaryOp::LogicalNot),
        "&" => Ok(UnaryOp::ReductionAnd),
        "~&" => Ok(UnaryOp::ReductionNand),
        "|" => Ok(UnaryOp::ReductionOr),
        "^" => Ok(UnaryOp::ReductionXor),
        _ => Err(unsupported("unary operator is not supported yet", None)),
    }
}

pub(super) fn lower_binary_operator(
    syntax_tree: &SyntaxTree,
    operator: &BinaryOperator,
) -> LowerResult<BinaryOp> {
    match symbol_text(syntax_tree, &operator.nodes.0)?.as_str() {
        "&" => Ok(BinaryOp::BitAnd),
        "|" => Ok(BinaryOp::BitOr),
        "^" => Ok(BinaryOp::BitXor),
        "<<" => Ok(BinaryOp::ShiftLeft),
        ">>" => Ok(BinaryOp::ShiftRight),
        ">>>" => Ok(BinaryOp::ArithmeticShiftRight),
        "&&" => Ok(BinaryOp::LogicalAnd),
        "||" => Ok(BinaryOp::LogicalOr),
        "==" => Ok(BinaryOp::Eq),
        "!=" => Ok(BinaryOp::NotEq),
        "<" => Ok(BinaryOp::Lt),
        "<=" => Ok(BinaryOp::LtEq),
        ">" => Ok(BinaryOp::Gt),
        ">=" => Ok(BinaryOp::GtEq),
        "+" => Ok(BinaryOp::Add),
        "-" => Ok(BinaryOp::Sub),
        "*" => Ok(BinaryOp::Mul),
        _ => Err(unsupported("binary operator is not supported yet", None)),
    }
}

pub(super) fn lower_constant_range(
    syntax_tree: &SyntaxTree,
    range: &ConstantRange,
    path: &Path,
    params: &[ParameterDecl],
    frozen_construct: &str,
) -> LowerResult<PackedRange> {
    Ok(PackedRange {
        msb: lower_usize_constant_expression_with_params(
            syntax_tree,
            &range.nodes.0,
            path,
            params,
            frozen_construct,
        )?,
        lsb: lower_usize_constant_expression_with_params(
            syntax_tree,
            &range.nodes.2,
            path,
            params,
            frozen_construct,
        )?,
    })
}

pub(super) fn lower_constant_part_select_range(
    syntax_tree: &SyntaxTree,
    range: &ConstantPartSelectRange,
    path: &Path,
) -> LowerResult<(usize, usize)> {
    match range {
        ConstantPartSelectRange::ConstantRange(range) => Ok((
            lower_usize_constant_expression(syntax_tree, &range.nodes.0, path)?,
            lower_usize_constant_expression(syntax_tree, &range.nodes.2, path)?,
        )),
        ConstantPartSelectRange::ConstantIndexedRange(_) => Err(unsupported(
            "indexed part selects are not supported yet",
            None,
        )),
    }
}

pub(super) fn lower_part_select_range(
    syntax_tree: &SyntaxTree,
    range: &PartSelectRange,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<(usize, usize)> {
    match range {
        PartSelectRange::ConstantRange(range) => Ok((
            lower_usize_constant_expression(syntax_tree, &range.nodes.0, path)?,
            lower_usize_constant_expression(syntax_tree, &range.nodes.2, path)?,
        )),
        PartSelectRange::IndexedRange(range) => {
            let base = lower_usize_expression(
                syntax_tree,
                &range.nodes.0,
                module,
                path,
                "an indexed part select",
            )?;
            let width = lower_usize_constant_expression_with_params(
                syntax_tree,
                &range.nodes.2,
                path,
                &module.parameters,
                "an indexed part select",
            )?;
            if width == 0 {
                return Err(unsupported(
                    "indexed part selects must have a positive width",
                    None,
                ));
            }
            match symbol_text(syntax_tree, &range.nodes.1)?.as_str() {
                "+:" => Ok((
                    base.checked_add(width - 1).ok_or_else(|| {
                        unsupported("indexed part select exceeds host limits", None)
                    })?,
                    base,
                )),
                "-:" => Ok((
                    base,
                    base.checked_sub(width - 1).ok_or_else(|| {
                        unsupported("indexed part select exceeds host limits", None)
                    })?,
                )),
                _ => Err(unsupported(
                    "indexed part select uses an unsupported operator",
                    None,
                )),
            }
        }
    }
}
