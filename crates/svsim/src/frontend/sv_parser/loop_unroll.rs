//! Procedural `for` loop unrolling — elaboration work done at lowering time.
//!
//! HIR has no loop statement and only constant selects, so `for` loops are
//! fully unrolled here: the loop variable is bound as a pseudo-parameter,
//! the body is lowered once per iteration, and the variable is substituted
//! with that iteration's literal. Bounds and steps are evaluated with the
//! module's *default* parameter values — instance overrides cannot reach an
//! already-unrolled body, which is exactly the freeze that
//! `ModuleSummary::frozen_parameters` records and the elaboration fence
//! rejects (step 2 of the 2026-07-06 architectural review). If HIR ever
//! grows a loop representation, this module is the code it replaces.

use super::*;

pub(super) fn lower_loop_statement(
    syntax_tree: &SyntaxTree,
    statement: &sv_parser::LoopStatement,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    match statement {
        sv_parser::LoopStatement::For(statement) => {
            lower_for_loop_statement(syntax_tree, statement, module, path)
        }
        _ => Err(unsupported(
            "loop statement is outside the current executable subset",
            span_of_node(path, statement),
        )),
    }
}

pub(super) fn lower_for_loop_statement(
    syntax_tree: &SyntaxTree,
    statement: &sv_parser::LoopStatementFor,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    let controls = &statement.nodes.1.nodes.1;
    let Some(init) = controls.0.as_ref() else {
        return Err(unsupported(
            "procedural `for` loops require an initialization assignment",
            span_of_node(path, statement),
        ));
    };
    let Some(cond_expr) = controls.2.as_ref() else {
        return Err(unsupported(
            "procedural `for` loops require a constant-bounded condition",
            None,
        ));
    };
    let Some(step) = controls.4.as_ref() else {
        return Err(unsupported(
            "procedural `for` loops require a step assignment",
            None,
        ));
    };

    let (loop_var, mut loop_value) =
        lower_for_loop_initialization(syntax_tree, init, module, path)?;
    let mut statements = Vec::new();

    for _ in 0..PROCEDURAL_FOR_UNROLL_LIMIT {
        let iteration_module = module_with_const_binding(module, &loop_var, &loop_value);
        let cond = lower_expression(syntax_tree, cond_expr, &iteration_module, path)?;
        if !const_eval_param_expr(
            &cond,
            &iteration_module.parameters,
            "a procedural `for` loop bound",
        )?
        .truthy()
        {
            return Ok(fold_loop_statements(statements));
        }

        let body =
            lower_statement_or_null(syntax_tree, &statement.nodes.2, &iteration_module, path)?;
        let body =
            substitute_stmt_ident(&body, &loop_var, &expr_from_const_eval_value(&loop_value));
        if !matches!(body, Stmt::Empty) {
            statements.push(body);
        }

        loop_value = lower_for_loop_step(
            syntax_tree,
            step,
            &iteration_module,
            path,
            &loop_var,
            &loop_value,
        )?;
    }

    Err(unsupported(
        "procedural `for` loop exceeds the supported unrolling limit",
        span_of_node(path, statement),
    ))
}

pub(super) fn lower_for_loop_initialization(
    syntax_tree: &SyntaxTree,
    init: &sv_parser::ForInitialization,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<(String, Value)> {
    match init {
        sv_parser::ForInitialization::ListOfVariableAssignments(assignments) => {
            let assignments = assignments.nodes.0.contents();
            let [assignment] = assignments.as_slice() else {
                return Err(unsupported(
                    "procedural `for` loops only support a single initialization assignment",
                    span_of_node(path, init),
                ));
            };
            lower_for_loop_variable_assignment(
                syntax_tree,
                assignment,
                module,
                path,
                "procedural `for` loop initialization",
            )
        }
        sv_parser::ForInitialization::Declaration(declaration) => {
            let declarations = declaration.nodes.0.contents();
            let [declaration] = declarations.as_slice() else {
                return Err(unsupported(
                    "procedural `for` loops only support a single initialization declaration",
                    span_of_node(path, init),
                ));
            };
            let assignments = declaration.nodes.2.contents();
            let [(identifier, _, expr)] = assignments.as_slice() else {
                return Err(unsupported(
                    "procedural `for` loops only support a single initialized loop variable",
                    span_of_node(path, init),
                ));
            };
            let (name, _) = identifier_name_from_node(syntax_tree, RefNode::from(identifier))
                .ok_or_else(|| {
                    unsupported(
                        "failed to determine loop variable name",
                        span_of_node(path, init),
                    )
                })?;
            Ok((
                name,
                normalize_for_loop_value(lower_const_eval_expression(
                    syntax_tree,
                    expr,
                    module,
                    path,
                )?),
            ))
        }
    }
}

pub(super) fn lower_for_loop_variable_assignment(
    syntax_tree: &SyntaxTree,
    assignment: &VariableAssignment,
    module: &ModuleSummary,
    path: &Path,
    context: &str,
) -> LowerResult<(String, Value)> {
    if symbol_text(syntax_tree, &assignment.nodes.1)? != "=" {
        return Err(unsupported(
            format!("{context} must use `=`"),
            span_of_node(path, assignment),
        ));
    }

    let LValue::Signal(name) =
        lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)?
    else {
        return Err(unsupported(
            format!("{context} must target a plain loop variable"),
            span_of_node(path, assignment),
        ));
    };

    Ok((
        name,
        normalize_for_loop_value(lower_const_eval_expression(
            syntax_tree,
            &assignment.nodes.2,
            module,
            path,
        )?),
    ))
}

pub(super) fn lower_for_loop_step(
    syntax_tree: &SyntaxTree,
    step: &sv_parser::ForStep,
    module: &ModuleSummary,
    path: &Path,
    loop_var: &str,
    loop_value: &Value,
) -> LowerResult<Value> {
    let assignments = step.nodes.0.contents();
    let [assignment] = assignments.as_slice() else {
        return Err(unsupported(
            "procedural `for` loops only support a single step assignment",
            span_of_node(path, step),
        ));
    };

    match assignment {
        sv_parser::ForStepAssignment::OperatorAssignment(assignment) => {
            lower_for_loop_operator_step(syntax_tree, assignment, module, path, loop_var)
        }
        sv_parser::ForStepAssignment::IncOrDecExpression(assignment) => {
            let (target, op) = match assignment.as_ref() {
                sv_parser::IncOrDecExpression::Prefix(assignment) => {
                    (&assignment.nodes.2, &assignment.nodes.0)
                }
                sv_parser::IncOrDecExpression::Suffix(assignment) => {
                    (&assignment.nodes.0, &assignment.nodes.2)
                }
            };
            let LValue::Signal(name) = lower_variable_lvalue(syntax_tree, target, module, path)?
            else {
                return Err(unsupported(
                    "procedural `for` loop steps must target a plain loop variable",
                    span_of_node(path, step),
                ));
            };
            if name != loop_var {
                return Err(unsupported(
                    "procedural `for` loop step must update the initialized loop variable",
                    None,
                ));
            }
            let loop_bits = loop_value.to_bit_value_checked().ok_or_else(|| {
                unsupported(
                    "procedural `for` loops require two-state loop values",
                    span_of_node(path, step),
                )
            })?;
            match symbol_text(syntax_tree, &op.nodes.0)?.as_str() {
                "++" => Ok(normalize_for_loop_value(Value::new_with_signed(
                    loop_bits.wrapping_add(&BitValue::from(1u64), loop_value.width),
                    loop_value.width,
                    loop_value.signed,
                ))),
                "--" => Ok(normalize_for_loop_value(Value::new_with_signed(
                    loop_bits.wrapping_sub(&BitValue::from(1u64), loop_value.width),
                    loop_value.width,
                    loop_value.signed,
                ))),
                _ => Err(unsupported(
                    "procedural `for` loop step uses an unsupported increment operator",
                    None,
                )),
            }
        }
        sv_parser::ForStepAssignment::FunctionSubroutineCall(_) => Err(unsupported(
            "procedural `for` loop step must be an assignment",
            None,
        )),
    }
}

pub(super) fn lower_for_loop_operator_step(
    syntax_tree: &SyntaxTree,
    assignment: &sv_parser::OperatorAssignment,
    module: &ModuleSummary,
    path: &Path,
    loop_var: &str,
) -> LowerResult<Value> {
    if symbol_text(syntax_tree, &assignment.nodes.1.nodes.0)? != "=" {
        return Err(unsupported(
            "procedural `for` loop step must use `=`",
            span_of_node(path, assignment),
        ));
    }

    let LValue::Signal(name) =
        lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)?
    else {
        return Err(unsupported(
            "procedural `for` loop step must target a plain loop variable",
            span_of_node(path, assignment),
        ));
    };
    if name != loop_var {
        return Err(unsupported(
            "procedural `for` loop step must update the initialized loop variable",
            None,
        ));
    }

    lower_const_eval_expression(syntax_tree, &assignment.nodes.2, module, path)
        .map(normalize_for_loop_value)
}

pub(super) fn lower_const_eval_expression(
    syntax_tree: &SyntaxTree,
    expr: &Expression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Value> {
    let lowered = lower_expression(syntax_tree, expr, module, path)?;
    const_eval_param_expr(
        &lowered,
        &module.parameters,
        "a procedural `for` loop control",
    )
    .map_err(|_| {
        unsupported(
            "procedural `for` loops require constant-bounded expressions",
            span_of_node(path, expr),
        )
    })
}

pub(super) fn module_with_const_binding(
    module: &ModuleSummary,
    name: &str,
    value: &Value,
) -> ModuleSummary {
    let mut module = module.clone();
    module.parameters.insert(
        0,
        ParameterDecl {
            name: name.into(),
            range: None,
            default_value: expr_from_const_eval_value(value),
            span: None,
        },
    );
    module
}

pub(super) fn fold_loop_statements(statements: Vec<Stmt>) -> Stmt {
    if statements.is_empty() {
        Stmt::Empty
    } else {
        Stmt::Block(statements)
    }
}

pub(super) fn normalize_for_loop_value(value: Value) -> Value {
    value.coerced_to(value.width.max(32))
}

pub(super) fn substitute_stmt_ident(stmt: &Stmt, name: &str, replacement: &Expr) -> Stmt {
    match stmt {
        Stmt::Empty => Stmt::Empty,
        Stmt::Block(statements) => Stmt::Block(
            statements
                .iter()
                .map(|statement| substitute_stmt_ident(statement, name, replacement))
                .collect(),
        ),
        Stmt::Assign { kind, target, expr } => Stmt::Assign {
            kind: kind.clone(),
            target: substitute_lvalue_ident(target, name, replacement),
            expr: substitute_expr_ident(expr, name, replacement),
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: substitute_expr_ident(cond, name, replacement),
            then_branch: Box::new(substitute_stmt_ident(then_branch, name, replacement)),
            else_branch: else_branch
                .as_ref()
                .map(|branch| Box::new(substitute_stmt_ident(branch, name, replacement))),
        },
        Stmt::Case {
            expr,
            items,
            default,
        } => Stmt::Case {
            expr: substitute_expr_ident(expr, name, replacement),
            items: items
                .iter()
                .map(|item| CaseStmtItem {
                    patterns: item
                        .patterns
                        .iter()
                        .map(|pattern| substitute_expr_ident(pattern, name, replacement))
                        .collect(),
                    body: substitute_stmt_ident(&item.body, name, replacement),
                })
                .collect(),
            default: default
                .as_ref()
                .map(|stmt| Box::new(substitute_stmt_ident(stmt, name, replacement))),
        },
    }
}

pub(super) fn substitute_lvalue_ident(lvalue: &LValue, name: &str, replacement: &Expr) -> LValue {
    match lvalue {
        LValue::Signal(signal) => LValue::Signal(signal.clone()),
        LValue::Concat(items) => LValue::Concat(
            items
                .iter()
                .map(|item| substitute_lvalue_ident(item, name, replacement))
                .collect(),
        ),
        LValue::BitSelect { signal, index } => LValue::BitSelect {
            signal: signal.clone(),
            index: *index,
        },
        LValue::PartSelect { signal, msb, lsb } => LValue::PartSelect {
            signal: signal.clone(),
            msb: *msb,
            lsb: *lsb,
        },
        LValue::MemoryElement { memory, index } => LValue::MemoryElement {
            memory: memory.clone(),
            index: Box::new(substitute_expr_ident(index, name, replacement)),
        },
    }
}

pub(super) fn substitute_expr_ident(expr: &Expr, name: &str, replacement: &Expr) -> Expr {
    match expr {
        Expr::Ident(ident) if ident == name => replacement.clone(),
        Expr::Ident(ident) => Expr::Ident(ident.clone()),
        Expr::Literal(literal) => Expr::Literal(literal.clone()),
        Expr::Concat(items) => Expr::Concat(
            items
                .iter()
                .map(|item| substitute_expr_ident(item, name, replacement))
                .collect(),
        ),
        Expr::Repeat { count, expr } => Expr::Repeat {
            count: *count,
            expr: Box::new(substitute_expr_ident(expr, name, replacement)),
        },
        Expr::MemoryRead { memory, index } => Expr::MemoryRead {
            memory: memory.clone(),
            index: Box::new(substitute_expr_ident(index, name, replacement)),
        },
        Expr::BitSelect { expr, index } => Expr::BitSelect {
            expr: Box::new(substitute_expr_ident(expr, name, replacement)),
            index: *index,
        },
        Expr::PartSelect { expr, msb, lsb } => Expr::PartSelect {
            expr: Box::new(substitute_expr_ident(expr, name, replacement)),
            msb: *msb,
            lsb: *lsb,
        },
        Expr::Unary { op, expr } => Expr::Unary {
            op: op.clone(),
            expr: Box::new(substitute_expr_ident(expr, name, replacement)),
        },
        Expr::Binary { left, op, right } => Expr::Binary {
            left: Box::new(substitute_expr_ident(left, name, replacement)),
            op: op.clone(),
            right: Box::new(substitute_expr_ident(right, name, replacement)),
        },
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => Expr::Ternary {
            cond: Box::new(substitute_expr_ident(cond, name, replacement)),
            when_true: Box::new(substitute_expr_ident(when_true, name, replacement)),
            when_false: Box::new(substitute_expr_ident(when_false, name, replacement)),
        },
    }
}

pub(super) fn expr_from_const_eval_value(value: &Value) -> Expr {
    let literal = Expr::Literal(NumericLiteral {
        bits: value.logic().bits().clone(),
        width: Some(value.width),
    });
    if value.signed {
        Expr::Unary {
            op: UnaryOp::Signed,
            expr: Box::new(literal),
        }
    } else {
        literal
    }
}
