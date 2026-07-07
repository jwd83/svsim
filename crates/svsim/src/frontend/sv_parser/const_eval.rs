//! Lowering-time constant evaluation and frozen-parameter recording.
//!
//! `const_eval_param_expr` is the single choke point: every successful call
//! bakes its result into HIR (or prunes/unrolls around it), evaluating
//! against parameter *defaults* — lowering runs once per module, before
//! instantiation. Each call site passes a construct label, and the
//! parameters an expression consumed are recorded into
//! `ModuleSummary::frozen_parameters`, which elaboration uses to reject
//! instance overrides that would contradict the frozen HIR (see the field's
//! docs in `hir.rs` and step 2 of the 2026-07-06 architectural review).

use super::*;

pub(super) fn lower_usize_expression(
    syntax_tree: &SyntaxTree,
    expr: &Expression,
    module: &ModuleSummary,
    path: &Path,
    frozen_construct: &str,
) -> LowerResult<usize> {
    let lowered = lower_expression(syntax_tree, expr, module, path)?;
    match const_eval_param_value(&lowered, &module.parameters, frozen_construct) {
        Ok(value) => Ok(value),
        Err(_) => Err(unsupported(
            "only constant bit and part select indices are supported",
            span_of_node(path, expr),
        )),
    }
}

pub(super) fn lower_usize_constant_expression(
    syntax_tree: &SyntaxTree,
    expr: &ConstantExpression,
    path: &Path,
) -> LowerResult<usize> {
    lower_usize_constant_expression_with_params(
        syntax_tree,
        expr,
        path,
        &[],
        "a constant expression",
    )
}

pub(super) fn lower_usize_constant_expression_with_params(
    syntax_tree: &SyntaxTree,
    expr: &ConstantExpression,
    path: &Path,
    params: &[ParameterDecl],
    frozen_construct: &str,
) -> LowerResult<usize> {
    let module = const_eval_module(params);
    let lowered = lower_constant_expression_to_expr(syntax_tree, expr, &module, path)?;
    const_eval_param_expr(&lowered, params, frozen_construct)
        .map_err(|diag| with_fallback_span(diag, span_of_node(path, expr)))?
        .to_usize_checked()
        .ok_or_else(|| {
            unsupported(
                "constant index exceeds host limits",
                span_of_node(path, expr),
            )
        })
}

pub(super) fn const_eval_module(params: &[ParameterDecl]) -> ModuleSummary {
    ModuleSummary {
        name: "<constant>".into(),
        style: ModuleDeclStyle::Ansi,
        span: None,
        ports: Vec::new(),
        parameters: params.to_vec(),
        signals: Vec::new(),
        memories: Vec::new(),
        continuous_assignments: Vec::new(),
        proc_blocks: Vec::new(),
        instantiations: Vec::new(),
        unsupported: Vec::new(),
        frozen_parameters: BTreeMap::new(),
    }
}

thread_local! {
    /// Active per-module collector for `ModuleSummary::frozen_parameters`.
    /// Installed by the module lowering drivers; single-threaded per module
    /// because a module is always lowered on one thread, start to finish.
    static FROZEN_PARAM_RECORDER: RefCell<Option<BTreeMap<String, String>>> =
        const { RefCell::new(None) };
}

/// Scope marker for frozen-parameter recording while one module is lowered.
pub(super) struct FrozenParamRecording;

impl FrozenParamRecording {
    pub(super) fn begin() -> Self {
        FROZEN_PARAM_RECORDER.with(|recorder| *recorder.borrow_mut() = Some(BTreeMap::new()));
        FrozenParamRecording
    }

    pub(super) fn finish(self) -> BTreeMap<String, String> {
        FROZEN_PARAM_RECORDER
            .with(|recorder| recorder.borrow_mut().take())
            .unwrap_or_default()
    }
}

pub(super) fn record_frozen_params(expr: &Expr, params: &[ParameterDecl], frozen_construct: &str) {
    if params.is_empty() {
        return;
    }
    let mut names = Vec::new();
    collect_expr_param_refs(expr, params, &mut names);
    if names.is_empty() {
        return;
    }
    FROZEN_PARAM_RECORDER.with(|recorder| {
        if let Some(map) = recorder.borrow_mut().as_mut() {
            for name in names {
                map.entry(name)
                    .or_insert_with(|| frozen_construct.to_string());
            }
        }
    });
}

pub(super) fn collect_expr_param_refs(
    expr: &Expr,
    params: &[ParameterDecl],
    names: &mut Vec<String>,
) {
    match expr {
        Expr::Ident(name) => {
            if params.iter().any(|param| param.name == *name) {
                names.push(name.clone());
            }
        }
        Expr::Literal(_) => {}
        Expr::Concat(exprs) => {
            for expr in exprs {
                collect_expr_param_refs(expr, params, names);
            }
        }
        Expr::Repeat { expr, .. } => collect_expr_param_refs(expr, params, names),
        Expr::MemoryRead { index, .. } => collect_expr_param_refs(index, params, names),
        Expr::BitSelect { expr, .. } => collect_expr_param_refs(expr, params, names),
        Expr::PartSelect { expr, .. } => collect_expr_param_refs(expr, params, names),
        Expr::Unary { expr, .. } => collect_expr_param_refs(expr, params, names),
        Expr::Binary { left, right, .. } => {
            collect_expr_param_refs(left, params, names);
            collect_expr_param_refs(right, params, names);
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            collect_expr_param_refs(cond, params, names);
            collect_expr_param_refs(when_true, params, names);
            collect_expr_param_refs(when_false, params, names);
        }
    }
}

/// The single choke point for lowering-time constant evaluation. Every
/// successful call bakes the result into HIR (or prunes/unrolls around it),
/// so any parameter the expression references is recorded as frozen under
/// the caller-supplied construct description.
pub(super) fn const_eval_param_expr(
    expr: &Expr,
    params: &[ParameterDecl],
    frozen_construct: &str,
) -> LowerResult<Value> {
    let module = const_eval_module(params);
    let values = resolve_parameter_defaults(params, &module)
        .map_err(|error| unsupported(error.to_string(), None))?;
    let value = eval_expr(expr, &module, &values, &HashMap::new())
        .map_err(|error| unsupported(error.to_string(), None))?;
    record_frozen_params(expr, params, frozen_construct);
    Ok(value)
}

pub(super) fn const_eval_param_value(
    expr: &Expr,
    params: &[ParameterDecl],
    frozen_construct: &str,
) -> LowerResult<usize> {
    const_eval_param_expr(expr, params, frozen_construct)?
        .to_usize_checked()
        .ok_or_else(|| unsupported("constant value exceeds host limits", None))
}
