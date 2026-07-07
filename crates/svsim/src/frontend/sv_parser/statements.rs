//! Procedural statement lowering: `always_comb`/`always_ff` constructs,
//! event controls, blocking/nonblocking assignments, begin/end blocks,
//! conditionals (with parameter-constant pruning via `const_eval`), case
//! statements, and inert subroutine calls.

use super::*;

pub(super) fn lower_initial_construct(
    syntax_tree: &SyntaxTree,
    construct: &InitialConstruct,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<()> {
    let body = lower_statement_or_null(syntax_tree, &construct.nodes.1, module, path)?;
    if stmt_is_inert(&body) {
        Ok(())
    } else {
        Err(unsupported(
            "initial constructs are not supported yet",
            None,
        ))
    }
}

pub(super) fn stmt_is_inert(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Empty => true,
        Stmt::Block(statements) => statements.iter().all(stmt_is_inert),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => stmt_is_inert(then_branch) && else_branch.as_deref().is_none_or(stmt_is_inert),
        Stmt::Case { items, default, .. } => {
            items.iter().all(|item| stmt_is_inert(&item.body))
                && default.as_deref().is_none_or(stmt_is_inert)
        }
        Stmt::Assign { .. } => false,
    }
}

pub(super) fn lower_always_construct(
    syntax_tree: &SyntaxTree,
    construct: &AlwaysConstruct,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<ProcBlock> {
    match &construct.nodes.0 {
        AlwaysKeyword::AlwaysComb(_) => Ok(ProcBlock {
            kind: ProcBlockKind::AlwaysComb,
            body: lower_statement(syntax_tree, &construct.nodes.1, module, path)?,
            span: None,
        }),
        AlwaysKeyword::Always(_) => {
            lower_always_generic(syntax_tree, &construct.nodes.1, module, path)
        }
        AlwaysKeyword::AlwaysLatch(_) => Err(unsupported(
            "`always_latch` blocks are not supported yet",
            None,
        )),
        AlwaysKeyword::AlwaysFf(_) => {
            let (clock, async_reset, body) =
                lower_always_ff_statement(syntax_tree, &construct.nodes.1, module, path)?;
            Ok(ProcBlock {
                kind: ProcBlockKind::AlwaysFf { clock, async_reset },
                body,
                span: None,
            })
        }
    }
}

/// Lower a Verilog-2001 `always @(...)` block by inspecting the sensitivity list:
/// - `@*` or `@(*)` → AlwaysComb
/// - `@(posedge clk)` → AlwaysFf { clock, async_reset: None }
/// - `@(posedge clk or posedge reset)` → AlwaysFf { clock, async_reset: Some(reset) }
pub(super) fn lower_always_generic(
    syntax_tree: &SyntaxTree,
    statement: &Statement,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<ProcBlock> {
    if statement.nodes.0.is_some() {
        return Err(unsupported(
            "named procedural blocks are not supported yet",
            None,
        ));
    }

    let StatementItem::ProceduralTimingControlStatement(timing_stmt) = &statement.nodes.2 else {
        return Err(unsupported(
            "`always` blocks must have a sensitivity list (e.g. `always @*` or `always @(posedge clk)`)",
            None,
        ));
    };

    let sv_parser::ProceduralTimingControl::EventControl(control) = &timing_stmt.nodes.0 else {
        return Err(unsupported(
            "`always` blocks only support event controls, not delays or cycle delays",
            None,
        ));
    };

    match control.as_ref() {
        // always @* or always @(*)  →  AlwaysComb
        sv_parser::EventControl::Asterisk(_) | sv_parser::EventControl::ParenAsterisk(_) => {
            let body = lower_statement_or_null(syntax_tree, &timing_stmt.nodes.1, module, path)?;
            Ok(ProcBlock {
                kind: ProcBlockKind::AlwaysComb,
                body,
                span: None,
            })
        }
        // always @(posedge clk)  →  AlwaysFf
        sv_parser::EventControl::EventExpression(expr) => {
            let (clock, async_reset) =
                lower_always_ff_event_expression(syntax_tree, &expr.nodes.1.nodes.1, module, path)?;
            let body = lower_statement_or_null(syntax_tree, &timing_stmt.nodes.1, module, path)?;
            Ok(ProcBlock {
                kind: ProcBlockKind::AlwaysFf { clock, async_reset },
                body,
                span: None,
            })
        }
        _ => Err(unsupported(
            "`always` blocks only support `@*`, `@(posedge <clock>)`, or `@(posedge <clock> or posedge <reset>)` sensitivity lists",
            None,
        )),
    }
}

pub(super) fn lower_always_ff_statement(
    syntax_tree: &SyntaxTree,
    statement: &Statement,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<(String, Option<String>, Stmt)> {
    if statement.nodes.0.is_some() {
        return Err(unsupported(
            "named procedural blocks are not supported yet",
            None,
        ));
    }

    let StatementItem::ProceduralTimingControlStatement(statement) = &statement.nodes.2 else {
        return Err(unsupported(
            "`always_ff` blocks must use a single event control statement",
            None,
        ));
    };

    let (clock, async_reset) = match &statement.nodes.0 {
        sv_parser::ProceduralTimingControl::EventControl(control) => {
            lower_always_ff_event_control(syntax_tree, control, module, path)?
        }
        _ => {
            return Err(unsupported(
                "`always_ff` blocks only support event controls",
                None,
            ));
        }
    };
    let body = lower_statement_or_null(syntax_tree, &statement.nodes.1, module, path)?;
    Ok((clock, async_reset, body))
}

pub(super) fn lower_always_ff_event_control(
    syntax_tree: &SyntaxTree,
    control: &sv_parser::EventControl,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<(String, Option<String>)> {
    match control {
        sv_parser::EventControl::EventExpression(expr) => {
            lower_always_ff_event_expression(syntax_tree, &expr.nodes.1.nodes.1, module, path)
        }
        _ => Err(unsupported(
            "`always_ff` blocks must use `@(posedge <clock>)` or `@(posedge <clock> or posedge <reset>)`",
            None,
        )),
    }
}

pub(super) fn lower_always_ff_event_expression(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::EventExpression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<(String, Option<String>)> {
    let mut signals = Vec::new();
    collect_always_ff_event_signals(syntax_tree, expr, module, path, &mut signals)?;

    match signals.len() {
        1 => Ok((signals.remove(0), None)),
        2 => {
            let mut first = signals.remove(0);
            let mut second = signals.remove(0);
            if looks_like_reset_signal(&first) && !looks_like_reset_signal(&second) {
                std::mem::swap(&mut first, &mut second);
            }
            Ok((first, Some(second)))
        }
        _ => Err(unsupported(
            "`always_ff` blocks currently support one clock edge and an optional async reset edge",
            None,
        )),
    }
}

pub(super) fn collect_always_ff_event_signals(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::EventExpression,
    module: &ModuleSummary,
    path: &Path,
    out: &mut Vec<String>,
) -> LowerResult<()> {
    match expr {
        sv_parser::EventExpression::Expression(expr) => {
            if !matches!(expr.nodes.0, Some(sv_parser::EdgeIdentifier::Posedge(_))) {
                return Err(unsupported(
                    "`always_ff` blocks currently require `posedge` event controls",
                    None,
                ));
            }
            if expr.nodes.2.is_some() {
                return Err(unsupported(
                    "`always_ff` event expressions with `iff` are not supported yet",
                    None,
                ));
            }

            let Expr::Ident(signal) = lower_expression(syntax_tree, &expr.nodes.1, module, path)?
            else {
                return Err(unsupported(
                    "`always_ff` event expressions must name local signals",
                    None,
                ));
            };
            out.push(signal);
            Ok(())
        }
        sv_parser::EventExpression::Or(expr) => {
            collect_always_ff_event_signals(syntax_tree, &expr.nodes.0, module, path, out)?;
            collect_always_ff_event_signals(syntax_tree, &expr.nodes.2, module, path, out)
        }
        sv_parser::EventExpression::Comma(expr) => {
            collect_always_ff_event_signals(syntax_tree, &expr.nodes.0, module, path, out)?;
            collect_always_ff_event_signals(syntax_tree, &expr.nodes.2, module, path, out)
        }
        sv_parser::EventExpression::Paren(expr) => {
            collect_always_ff_event_signals(syntax_tree, &expr.nodes.0.nodes.1, module, path, out)
        }
        _ => Err(unsupported(
            "`always_ff` blocks only support edge-triggered signal event expressions",
            None,
        )),
    }
}

pub(super) fn looks_like_reset_signal(name: &str) -> bool {
    matches!(name, "reset" | "rst" | "resetn" | "rst_n" | "rstn")
        || name.starts_with("reset_")
        || name.starts_with("rst_")
}

pub(super) fn lower_statement(
    syntax_tree: &SyntaxTree,
    statement: &Statement,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    if statement.nodes.0.is_some() {
        return Err(unsupported(
            "named procedural blocks are not supported yet",
            None,
        ));
    }

    match &statement.nodes.2 {
        StatementItem::BlockingAssignment(assignment) => {
            lower_blocking_assignment(syntax_tree, &assignment.0, module, path)
        }
        StatementItem::NonblockingAssignment(assignment) => {
            lower_nonblocking_assignment(syntax_tree, &assignment.0, module, path)
        }
        StatementItem::SeqBlock(block) => lower_seq_block(syntax_tree, block, module, path),
        StatementItem::ConditionalStatement(statement) => {
            lower_conditional_statement(syntax_tree, statement, module, path)
        }
        StatementItem::CaseStatement(statement) => {
            lower_case_statement(syntax_tree, statement, module, path)
        }
        StatementItem::LoopStatement(statement) => {
            lower_loop_statement(syntax_tree, statement, module, path)
        }
        StatementItem::SubroutineCallStatement(statement) => {
            lower_subroutine_call_statement(syntax_tree, statement)
        }
        _ => Err(unsupported(
            "statement is outside the current executable subset",
            None,
        )),
    }
}

pub(super) fn lower_statement_or_null(
    syntax_tree: &SyntaxTree,
    statement: &StatementOrNull,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    match statement {
        StatementOrNull::Statement(statement) => {
            lower_statement(syntax_tree, statement, module, path)
        }
        StatementOrNull::Attribute(_) => Ok(Stmt::Empty),
    }
}

pub(super) fn lower_subroutine_call_statement(
    syntax_tree: &SyntaxTree,
    statement: &sv_parser::SubroutineCallStatement,
) -> LowerResult<Stmt> {
    match statement {
        sv_parser::SubroutineCallStatement::SubroutineCall(call) => {
            if is_inert_subroutine_call(syntax_tree, &call.0) {
                Ok(Stmt::Empty)
            } else {
                Err(unsupported(
                    "subroutine call statements are not supported yet",
                    None,
                ))
            }
        }
        sv_parser::SubroutineCallStatement::Function(_) => Err(unsupported(
            "subroutine call statements are not supported yet",
            None,
        )),
    }
}

pub(super) fn is_inert_subroutine_call(
    syntax_tree: &SyntaxTree,
    call: &sv_parser::SubroutineCall,
) -> bool {
    match call {
        sv_parser::SubroutineCall::TfCall(call) => {
            inert_task_name(syntax_tree, &call.nodes.0).as_deref() == Some("empty_statement")
        }
        sv_parser::SubroutineCall::SystemTfCall(call) => {
            inert_system_tf_name(syntax_tree, call).as_deref() == Some("$display")
        }
        _ => false,
    }
}

pub(super) fn inert_task_name(
    syntax_tree: &SyntaxTree,
    identifier: &sv_parser::PsOrHierarchicalTfIdentifier,
) -> Option<String> {
    match identifier {
        sv_parser::PsOrHierarchicalTfIdentifier::PackageScope(identifier) => {
            identifier_name_from_node(syntax_tree, RefNode::from(&identifier.nodes.1.nodes.0))
                .map(|(name, _)| name)
        }
        sv_parser::PsOrHierarchicalTfIdentifier::HierarchicalTfIdentifier(identifier) => {
            lower_hierarchical_identifier(syntax_tree, &identifier.nodes.0, "task calls")
                .ok()
                .map(|(name, _)| name)
        }
    }
}

pub(super) fn inert_system_tf_name(
    syntax_tree: &SyntaxTree,
    call: &sv_parser::SystemTfCall,
) -> Option<String> {
    let identifier = match call {
        sv_parser::SystemTfCall::ArgOptionl(call) => &call.nodes.0,
        sv_parser::SystemTfCall::ArgDataType(call) => &call.nodes.0,
        sv_parser::SystemTfCall::ArgExpression(call) => &call.nodes.0,
    };
    syntax_tree.get_str(&identifier.nodes.0).map(str::to_owned)
}

pub(super) fn is_inert_task_declaration(
    _syntax_tree: &SyntaxTree,
    decl: &sv_parser::TaskDeclaration,
) -> bool {
    match &decl.nodes.2 {
        sv_parser::TaskBodyDeclaration::WithoutPort(body) => {
            body.nodes.0.is_none()
                && body.nodes.3.is_empty()
                && body.nodes.4.len() == 1
                && matches!(
                    &body.nodes.4[0],
                    StatementOrNull::Statement(statement)
                        if statement.nodes.0.is_none()
                            && statement.nodes.1.is_empty()
                            && matches!(
                                &statement.nodes.2,
                                StatementItem::SeqBlock(block)
                                    if block.nodes.1.is_none()
                                        && block.nodes.2.is_empty()
                                        && block.nodes.3.is_empty()
                                        && block.nodes.5.is_none()
                            )
                )
        }
        sv_parser::TaskBodyDeclaration::WithPort(_) => false,
    }
}

pub(super) fn lower_blocking_assignment(
    syntax_tree: &SyntaxTree,
    assignment: &sv_parser::BlockingAssignment,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    match assignment {
        sv_parser::BlockingAssignment::OperatorAssignment(assignment) => {
            if symbol_text(syntax_tree, &assignment.nodes.1.nodes.0)? != "=" {
                return Err(unsupported(
                    "compound blocking assignments are not supported yet",
                    None,
                ));
            }
            Ok(Stmt::Assign {
                kind: AssignmentKind::Blocking,
                target: lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)?,
                expr: lower_expression(syntax_tree, &assignment.nodes.2, module, path)?,
            })
        }
        sv_parser::BlockingAssignment::Variable(_) => Err(unsupported(
            "blocking assignments with timing controls are not supported yet",
            None,
        )),
        _ => Err(unsupported(
            "blocking assignment is outside the current executable subset",
            None,
        )),
    }
}

pub(super) fn lower_nonblocking_assignment(
    syntax_tree: &SyntaxTree,
    assignment: &sv_parser::NonblockingAssignment,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    if assignment.nodes.2.is_some() {
        return Err(unsupported(
            "nonblocking assignments with timing controls are not supported yet",
            None,
        ));
    }
    if symbol_text(syntax_tree, &assignment.nodes.1)? != "<=" {
        return Err(unsupported(
            "compound nonblocking assignments are not supported yet",
            None,
        ));
    }

    Ok(Stmt::Assign {
        kind: AssignmentKind::Nonblocking,
        target: lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)?,
        expr: lower_expression(syntax_tree, &assignment.nodes.3, module, path)?,
    })
}

pub(super) fn lower_seq_block(
    syntax_tree: &SyntaxTree,
    block: &SeqBlock,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    if block.nodes.1.is_some() || block.nodes.5.is_some() {
        return Err(unsupported(
            "named begin/end blocks are not supported yet",
            None,
        ));
    }

    let mut statements = Vec::new();
    for declaration in &block.nodes.2 {
        statements.push(lower_block_item_declaration_stmt(
            syntax_tree,
            declaration,
            module,
            path,
        )?);
    }
    for statement in &block.nodes.3 {
        statements.push(lower_statement_or_null(
            syntax_tree,
            statement,
            module,
            path,
        )?);
    }

    Ok(Stmt::Block(statements))
}

pub(super) fn lower_block_item_declaration_stmt(
    syntax_tree: &SyntaxTree,
    declaration: &sv_parser::BlockItemDeclaration,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    let sv_parser::BlockItemDeclaration::Data(declaration) = declaration else {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            None,
        ));
    };

    let DataDeclaration::Variable(declaration) = &declaration.nodes.1 else {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            None,
        ));
    };

    let assignments = declaration.nodes.4.nodes.0.contents();
    let [assignment] = assignments.as_slice() else {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            None,
        ));
    };
    let VariableDeclAssignment::Variable(assignment) = assignment else {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            None,
        ));
    };
    if !assignment.nodes.1.is_empty() {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            None,
        ));
    }

    let (name, locate) = identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
        .ok_or_else(|| unsupported("failed to determine procedural declaration name", None))?;
    if name == "empty_statement" && assignment.nodes.2.is_none() {
        return Ok(Stmt::Empty);
    }
    if module.signal_width(&name).is_none() {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            Some(span_from_locate(path, locate)),
        ));
    }

    let Some((_, expr)) = assignment.nodes.2.as_ref() else {
        return Err(unsupported(
            "procedural blocks with local declarations are not supported yet",
            None,
        ));
    };

    Ok(Stmt::Assign {
        kind: AssignmentKind::Blocking,
        target: LValue::Signal(name),
        expr: lower_expression(syntax_tree, expr, module, path)?,
    })
}

pub(super) fn lower_conditional_statement(
    syntax_tree: &SyntaxTree,
    statement: &ConditionalStatement,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    if statement.nodes.0.is_some() {
        return Err(unsupported(
            "`unique`/`priority` procedural conditionals are not supported yet",
            None,
        ));
    }

    let cond = lower_cond_predicate(syntax_tree, &statement.nodes.2.nodes.1, module, path)?;
    if let Ok(value) = const_eval_param_expr(
        &cond,
        &module.parameters,
        "a constant-folded `if` condition",
    ) {
        return if value.truthy() {
            lower_statement_or_null(syntax_tree, &statement.nodes.3, module, path)
        } else {
            lower_conditional_else_chain(
                syntax_tree,
                &statement.nodes.4,
                statement.nodes.5.as_ref().map(|(_, branch)| branch),
                module,
                path,
            )
        };
    }

    let else_branch = lower_conditional_else_chain(
        syntax_tree,
        &statement.nodes.4,
        statement.nodes.5.as_ref().map(|(_, branch)| branch),
        module,
        path,
    )?;

    Ok(Stmt::If {
        cond,
        then_branch: Box::new(lower_statement_or_null(
            syntax_tree,
            &statement.nodes.3,
            module,
            path,
        )?),
        else_branch: (!stmt_is_inert(&else_branch)).then_some(Box::new(else_branch)),
    })
}

pub(super) fn lower_conditional_else_chain(
    syntax_tree: &SyntaxTree,
    else_ifs: &[(Keyword, Keyword, Paren<CondPredicate>, StatementOrNull)],
    final_else: Option<&StatementOrNull>,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    let Some((_, _, predicate, branch)) = else_ifs.first() else {
        return final_else
            .map(|branch| lower_statement_or_null(syntax_tree, branch, module, path))
            .transpose()?
            .map_or(Ok(Stmt::Empty), Ok);
    };

    let cond = lower_cond_predicate(syntax_tree, &predicate.nodes.1, module, path)?;
    if let Ok(value) = const_eval_param_expr(
        &cond,
        &module.parameters,
        "a constant-folded `if` condition",
    ) {
        return if value.truthy() {
            lower_statement_or_null(syntax_tree, branch, module, path)
        } else {
            lower_conditional_else_chain(syntax_tree, &else_ifs[1..], final_else, module, path)
        };
    }

    let tail = lower_conditional_else_chain(syntax_tree, &else_ifs[1..], final_else, module, path)?;

    Ok(Stmt::If {
        cond,
        then_branch: Box::new(lower_statement_or_null(syntax_tree, branch, module, path)?),
        else_branch: (!stmt_is_inert(&tail)).then_some(Box::new(tail)),
    })
}

pub(super) fn lower_case_statement(
    syntax_tree: &SyntaxTree,
    statement: &CaseStatement,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    let (keyword, expr, first_item, rest_items) = match statement {
        CaseStatement::Normal(statement) => (
            &statement.nodes.1,
            &statement.nodes.2.nodes.1.nodes.0,
            &statement.nodes.3,
            &statement.nodes.4,
        ),
        CaseStatement::Matches(_) | CaseStatement::Inside(_) => {
            return Err(unsupported(
                "only plain `case` statements are supported yet",
                None,
            ));
        }
    };

    match keyword {
        sv_parser::CaseKeyword::Case(_) => {}
        sv_parser::CaseKeyword::Casez(_) | sv_parser::CaseKeyword::Casex(_) => {
            return Err(unsupported("`casez`/`casex` are not supported yet", None));
        }
    }

    let mut items = Vec::new();
    let mut default = None;
    lower_case_item(
        syntax_tree,
        first_item,
        module,
        path,
        &mut items,
        &mut default,
    )?;
    for item in rest_items {
        lower_case_item(syntax_tree, item, module, path, &mut items, &mut default)?;
    }

    Ok(Stmt::Case {
        expr: lower_expression(syntax_tree, expr, module, path)?,
        items,
        default,
    })
}

pub(super) fn lower_case_item(
    syntax_tree: &SyntaxTree,
    item: &SvCaseItem,
    module: &ModuleSummary,
    path: &Path,
    items: &mut Vec<CaseStmtItem>,
    default: &mut Option<Box<Stmt>>,
) -> LowerResult<()> {
    match item {
        SvCaseItem::NonDefault(item) => {
            let mut patterns = Vec::new();
            for expr in item.nodes.0.contents() {
                patterns.push(lower_expression(syntax_tree, &expr.nodes.0, module, path)?);
            }
            items.push(CaseStmtItem {
                patterns,
                body: lower_statement_or_null(syntax_tree, &item.nodes.2, module, path)?,
            });
        }
        SvCaseItem::Default(item) => {
            if default.is_some() {
                return Err(unsupported(
                    "multiple default case items are not supported",
                    None,
                ));
            }
            *default = Some(Box::new(lower_statement_or_null(
                syntax_tree,
                &item.nodes.2,
                module,
                path,
            )?));
        }
    }

    Ok(())
}
