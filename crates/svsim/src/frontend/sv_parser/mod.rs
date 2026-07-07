use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use sv_parser::{
    AlwaysConstruct, AlwaysKeyword, AnsiPortDeclaration, BinaryOperator, CaseItem as SvCaseItem,
    CaseStatement, CondPredicate, ConditionalStatement, ConstantExpression,
    ConstantPartSelectRange, ConstantRange, ConstantSelect, ContinuousAssign, DataDeclaration,
    DataType, DataTypeOrImplicit, Define, Defines, Expression, FunctionSubroutineCall,
    HierarchicalIdentifier, ImplicitDataType, InitialConstruct, Keyword,
    ListOfParameterAssignments, ListOfPortConnections, LocalParameterDeclaration, Locate,
    ModuleDeclarationAnsi, ModuleDeclarationNonansi, ModuleInstantiation, ModuleOrGenerateItem,
    ModuleOrGenerateItemDeclaration, NamedPortConnection, NetDeclaration, NetLvalue,
    NonPortModuleItem, PackageOrGenerateItemDeclaration, ParameterDeclaration,
    ParameterPortDeclaration, ParameterPortList, Paren, PartSelectRange, PortDirection, Primary,
    PsOrHierarchicalNetIdentifier, RefNode, Select, SeqBlock, Statement, StatementItem,
    StatementOrNull, SyntaxTree, UnaryOperator, UnpackedDimension, VariableAssignment,
    VariableDeclAssignment, VariableDimension, VariableLvalue, VariablePortType, parse_sv,
    parse_sv_str, unwrap_node,
};

use crate::bit_value::BitValue;
use crate::diag::{Diagnostic, Error, Result, SourceSpan};
use crate::expr_eval::{Value, eval_expr, resolve_parameter_defaults};
use crate::hir::{
    AssignmentKind, BinaryOp, CaseStmtItem, ContinuousAssign as HirContinuousAssign, Expr, LValue,
    MemoryDecl, ModuleDeclStyle, ModuleInstanceSummary, ModuleSummary,
    NamedParameterAssign as HirNamedParameterAssign, NamedPortConnection as HirNamedPortConnection,
    NetKind, NumericLiteral, PackedRange, ParameterDecl, PortDecl,
    PortDirection as HirPortDirection, ProcBlock, ProcBlockKind, SignalDecl, SourceFile, Stmt,
    StorageKind, UnaryOp,
};
use crate::logic_value::{LogicBit, LogicBits};

type LowerResult<T> = std::result::Result<T, Diagnostic>;
const PROCEDURAL_FOR_UNROLL_LIMIT: usize = 16_384;

#[derive(Debug, Default)]
struct LoweredDeclarations {
    signals: Vec<SignalDecl>,
    memories: Vec<MemoryDecl>,
}

#[derive(Debug, Default)]
struct LoweredNetDeclarations {
    signals: Vec<SignalDecl>,
    initializers: Vec<HirContinuousAssign>,
}

#[derive(Debug, Clone, Copy)]
struct AnsiPortContext {
    direction: HirPortDirection,
    storage: StorageKind,
    range: Option<PackedRange>,
}

#[derive(Debug, Clone, Default)]
pub struct SvParserFrontend {
    include_paths: Vec<PathBuf>,
}

impl SvParserFrontend {
    pub fn new(include_paths: Vec<PathBuf>) -> Self {
        Self { include_paths }
    }

    pub fn parse_file(&self, path: &Path) -> Result<SourceFile> {
        let defines: Defines = HashMap::<String, Option<Define>>::new();
        let include_paths = self.include_paths_for(path);
        let (syntax_tree, _) =
            parse_sv(path, &defines, &include_paths, false, false).map_err(|error| {
                Error::Parse(format!("failed to parse {}: {error}", path.display()))
            })?;

        lower_source_file(&syntax_tree, path)
    }

    pub fn parse_str(&self, virtual_path: impl AsRef<Path>, source: &str) -> Result<SourceFile> {
        let path = virtual_path.as_ref();
        let defines: Defines = HashMap::<String, Option<Define>>::new();
        let include_paths = self.include_paths_for(path);
        let (syntax_tree, _) = parse_sv_str(source, path, &defines, &include_paths, false, false)
            .map_err(|error| {
            Error::Parse(format!("failed to parse {}: {error}", path.display()))
        })?;

        lower_source_file(&syntax_tree, path)
    }

    fn include_paths_for(&self, path: &Path) -> Vec<PathBuf> {
        let mut include_paths = Vec::with_capacity(self.include_paths.len() + 1);
        if let Some(parent) = path.parent() {
            include_paths.push(parent.to_path_buf());
        }
        include_paths.extend(self.include_paths.iter().cloned());
        include_paths
    }
}

fn lower_source_file(syntax_tree: &SyntaxTree, path: &Path) -> Result<SourceFile> {
    let mut modules = Vec::new();
    for node in syntax_tree {
        match node {
            RefNode::ModuleDeclarationAnsi(decl) => {
                modules.push(lower_ansi_module(syntax_tree, decl, path)?);
            }
            RefNode::ModuleDeclarationNonansi(decl) => {
                modules.push(lower_nonansi_module(syntax_tree, decl, path)?);
            }
            _ => {}
        }
    }

    Ok(SourceFile {
        path: path.to_path_buf(),
        modules,
    })
}

fn lower_ansi_module(
    syntax_tree: &SyntaxTree,
    decl: &ModuleDeclarationAnsi,
    path: &Path,
) -> Result<ModuleSummary> {
    let (name, locate) =
        identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.0.nodes.3)).ok_or_else(
            || {
                Error::Parse(format!(
                    "failed to determine module name in {}",
                    path.display()
                ))
            },
        )?;
    let recording = FrozenParamRecording::begin();
    let mut module = ModuleSummary {
        name,
        style: ModuleDeclStyle::Ansi,
        span: Some(span_from_locate(path, locate)),
        ports: Vec::new(),
        parameters: Vec::new(),
        signals: Vec::new(),
        memories: Vec::new(),
        continuous_assignments: Vec::new(),
        proc_blocks: Vec::new(),
        instantiations: Vec::new(),
        unsupported: Vec::new(),
        frozen_parameters: BTreeMap::new(),
    };

    if let Some(param_list) = decl.nodes.0.nodes.5.as_ref() {
        lower_parameter_port_list(syntax_tree, param_list, path, &mut module);
    }

    if let Some(port_decls) = decl.nodes.0.nodes.6.as_ref() {
        let mut context = None;
        if let Some(list) = port_decls.nodes.0.nodes.1.as_ref() {
            for port_decl in list.contents() {
                match lower_ansi_port_declaration(
                    syntax_tree,
                    &port_decl.1,
                    path,
                    context,
                    &module.parameters,
                ) {
                    Ok((port, next_context)) => {
                        module.ports.push(port);
                        context = Some(next_context);
                    }
                    Err(diag) => module.unsupported.push(diag),
                }
            }
        }
    }

    for item in &decl.nodes.2 {
        lower_non_port_module_item(syntax_tree, item, path, &mut module);
    }

    module.frozen_parameters = recording.finish();
    Ok(module)
}

fn lower_nonansi_module(
    syntax_tree: &SyntaxTree,
    decl: &ModuleDeclarationNonansi,
    path: &Path,
) -> Result<ModuleSummary> {
    let (name, locate) =
        identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.0.nodes.3)).ok_or_else(
            || {
                Error::Parse(format!(
                    "failed to determine module name in {}",
                    path.display()
                ))
            },
        )?;
    let recording = FrozenParamRecording::begin();
    let mut module = ModuleSummary {
        name,
        style: ModuleDeclStyle::NonAnsi,
        span: Some(span_from_locate(path, locate)),
        ports: Vec::new(),
        parameters: Vec::new(),
        signals: Vec::new(),
        memories: Vec::new(),
        continuous_assignments: Vec::new(),
        proc_blocks: Vec::new(),
        instantiations: Vec::new(),
        unsupported: vec![Diagnostic {
            message: "non-ANSI modules are parsed but not lowered into the executable subset yet"
                .into(),
            span: Some(span_from_locate(path, locate)),
        }],
        frozen_parameters: BTreeMap::new(),
    };

    for item in &decl.nodes.2 {
        if let sv_parser::ModuleItem::NonPortModuleItem(item) = item {
            lower_non_port_module_item(syntax_tree, item, path, &mut module);
        }
    }

    module.frozen_parameters = recording.finish();
    Ok(module)
}

fn lower_non_port_module_item(
    syntax_tree: &SyntaxTree,
    item: &NonPortModuleItem,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match item {
        NonPortModuleItem::GenerateRegion(region) => {
            lower_generate_region(syntax_tree, region, path, module);
        }
        NonPortModuleItem::ModuleOrGenerateItem(item) => {
            lower_module_or_generate_item(syntax_tree, item, path, module);
        }
        NonPortModuleItem::TimeunitsDeclaration(_) => {}
        _ => module.unsupported.push(Diagnostic {
            message: "module item is outside the current executable subset".into(),
            span: module.span.clone(),
        }),
    }
}

fn lower_parameter_port_list(
    syntax_tree: &SyntaxTree,
    list: &ParameterPortList,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match list {
        ParameterPortList::Assignment(list) => {
            // First: the initial ListOfParamAssignments (bare assignments inheriting `parameter`)
            for assignment in list.nodes.1.nodes.1.0.nodes.0.contents() {
                match lower_param_assignment(syntax_tree, assignment, None, module, path) {
                    Ok(param) => module.parameters.push(param),
                    Err(diag) => module.unsupported.push(diag),
                }
            }
            // Then: subsequent ParameterPortDeclaration entries
            for (_, decl) in &list.nodes.1.nodes.1.1 {
                lower_parameter_port_declaration(syntax_tree, decl, path, module);
            }
        }
        ParameterPortList::Declaration(list) => {
            for decl in list.nodes.1.nodes.1.contents() {
                lower_parameter_port_declaration(syntax_tree, decl, path, module);
            }
        }
        ParameterPortList::Empty(_) => {}
    }
}

fn lower_parameter_port_declaration(
    syntax_tree: &SyntaxTree,
    decl: &ParameterPortDeclaration,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match decl {
        ParameterPortDeclaration::ParameterDeclaration(decl) => {
            lower_parameter_or_localparam_declaration_into(
                syntax_tree,
                ParameterOrLocal::from_parameter_declaration(decl),
                path,
                module,
            );
        }
        ParameterPortDeclaration::LocalParameterDeclaration(decl) => {
            lower_parameter_or_localparam_declaration_into(
                syntax_tree,
                ParameterOrLocal::from_localparam_declaration(decl),
                path,
                module,
            );
        }
        ParameterPortDeclaration::ParamList(decl) => {
            let range =
                match lower_data_type_range(syntax_tree, &decl.nodes.0, path, &module.parameters) {
                    Ok(range) => range,
                    Err(diag) => {
                        module.unsupported.push(diag);
                        return;
                    }
                };
            for assignment in decl.nodes.1.nodes.0.contents() {
                match lower_param_assignment(syntax_tree, assignment, range, module, path) {
                    Ok(param) => module.parameters.push(param),
                    Err(diag) => module.unsupported.push(diag),
                }
            }
        }
        ParameterPortDeclaration::TypeList(_) => {
            module.unsupported.push(Diagnostic {
                message: "type parameter declarations are not supported yet".into(),
                span: None,
            });
        }
    }
}

fn lower_parameter_or_localparam_body(
    syntax_tree: &SyntaxTree,
    data_type: &DataTypeOrImplicit,
    assignments: &sv_parser::ListOfParamAssignments,
    path: &Path,
    module: &mut ModuleSummary,
) {
    let range =
        match lower_data_type_or_implicit_range(syntax_tree, data_type, path, &module.parameters) {
            Ok(r) => r,
            Err(diag) => {
                module.unsupported.push(diag);
                return;
            }
        };
    for assignment in assignments.nodes.0.contents() {
        match lower_param_assignment(syntax_tree, assignment, range, module, path) {
            Ok(param) => module.parameters.push(param),
            Err(diag) => module.unsupported.push(diag),
        }
    }
}

fn lower_parameter_or_localparam_declaration_into(
    syntax_tree: &SyntaxTree,
    pol: ParameterOrLocal<'_>,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match pol {
        ParameterOrLocal::Param {
            data_type,
            assignments,
        } => {
            lower_parameter_or_localparam_body(syntax_tree, data_type, assignments, path, module);
        }
        ParameterOrLocal::TypeParam => {
            module.unsupported.push(Diagnostic {
                message: "type parameter declarations are not supported yet".into(),
                span: None,
            });
        }
    }
}

/// Helper to unify parameter/localparam declaration shapes.
enum ParameterOrLocal<'a> {
    Param {
        data_type: &'a DataTypeOrImplicit,
        assignments: &'a sv_parser::ListOfParamAssignments,
    },
    TypeParam,
}

impl<'a> ParameterOrLocal<'a> {
    fn from_parameter_declaration(decl: &'a ParameterDeclaration) -> Self {
        match decl {
            ParameterDeclaration::Param(d) => ParameterOrLocal::Param {
                data_type: &d.nodes.1,
                assignments: &d.nodes.2,
            },
            ParameterDeclaration::Type(_) => ParameterOrLocal::TypeParam,
        }
    }

    fn from_localparam_declaration(decl: &'a LocalParameterDeclaration) -> Self {
        match decl {
            LocalParameterDeclaration::Param(d) => ParameterOrLocal::Param {
                data_type: &d.nodes.1,
                assignments: &d.nodes.2,
            },
            LocalParameterDeclaration::Type(_) => ParameterOrLocal::TypeParam,
        }
    }
}

fn lower_param_assignment(
    syntax_tree: &SyntaxTree,
    assignment: &sv_parser::ParamAssignment,
    range: Option<PackedRange>,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<ParameterDecl> {
    let (name, locate) = identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
        .ok_or_else(|| unsupported("failed to determine parameter name", None))?;

    if !assignment.nodes.1.is_empty() {
        return Err(unsupported(
            "parameter declarations with unpacked dimensions are not supported yet",
            None,
        ));
    }

    let (_, const_param_expr) = assignment.nodes.2.as_ref().ok_or_else(|| {
        unsupported(
            "parameter declarations without a default value are not supported yet",
            None,
        )
    })?;

    let default_value =
        lower_constant_param_expression(syntax_tree, const_param_expr, module, path)?;

    Ok(ParameterDecl {
        name,
        range,
        default_value,
        span: Some(span_from_locate(path, locate)),
    })
}

fn lower_constant_param_expression(
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

fn lower_constant_mintypmax_to_expr(
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

fn lower_constant_expression_to_expr(
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

fn lower_constant_primary_to_expr(
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

fn lower_module_or_generate_item(
    syntax_tree: &SyntaxTree,
    item: &ModuleOrGenerateItem,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match item {
        ModuleOrGenerateItem::Module(item) => {
            match lower_module_instantiation(syntax_tree, &item.nodes.1, module, path) {
                Ok(instantiations) => module.instantiations.extend(instantiations),
                Err(diag) => module.unsupported.push(diag),
            }
        }
        ModuleOrGenerateItem::ModuleItem(item) => match &item.nodes.1 {
            sv_parser::ModuleCommonItem::ModuleOrGenerateItemDeclaration(decl) => {
                lower_module_declaration_item(syntax_tree, decl, path, module);
            }
            sv_parser::ModuleCommonItem::ContinuousAssign(assign) => {
                match lower_continuous_assign(syntax_tree, assign, module, path) {
                    Ok(assignments) => module.continuous_assignments.extend(assignments),
                    Err(diag) => module.unsupported.push(diag),
                }
            }
            sv_parser::ModuleCommonItem::AlwaysConstruct(construct) => {
                match lower_always_construct(syntax_tree, construct, module, path) {
                    Ok(block) => module.proc_blocks.push(block),
                    Err(diag) => module.unsupported.push(diag),
                }
            }
            sv_parser::ModuleCommonItem::InitialConstruct(construct) => {
                if let Err(diag) = lower_initial_construct(syntax_tree, construct, module, path) {
                    module.unsupported.push(diag);
                }
            }
            sv_parser::ModuleCommonItem::ConditionalGenerateConstruct(construct) => {
                lower_conditional_generate_construct(syntax_tree, construct, path, module);
            }
            _ => module.unsupported.push(Diagnostic {
                message: "module item is outside the current executable subset".into(),
                span: module.span.clone(),
            }),
        },
        _ => module.unsupported.push(Diagnostic {
            message: "generate item is outside the current executable subset".into(),
            span: module.span.clone(),
        }),
    }
}

fn lower_initial_construct(
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

fn stmt_is_inert(stmt: &Stmt) -> bool {
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

fn lower_generate_region(
    syntax_tree: &SyntaxTree,
    region: &sv_parser::GenerateRegion,
    path: &Path,
    module: &mut ModuleSummary,
) {
    for item in &region.nodes.1 {
        lower_generate_item(syntax_tree, item, path, module);
    }
}

fn lower_generate_item(
    syntax_tree: &SyntaxTree,
    item: &sv_parser::GenerateItem,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match item {
        sv_parser::GenerateItem::ModuleOrGenerateItem(item) => {
            lower_module_or_generate_item(syntax_tree, item, path, module);
        }
        _ => module.unsupported.push(Diagnostic {
            message: "generate item is outside the current executable subset".into(),
            span: module.span.clone(),
        }),
    }
}

fn lower_generate_block(
    syntax_tree: &SyntaxTree,
    block: &sv_parser::GenerateBlock,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match block {
        sv_parser::GenerateBlock::GenerateItem(item) => {
            lower_generate_item(syntax_tree, item, path, module);
        }
        sv_parser::GenerateBlock::Multiple(block) => {
            for item in &block.nodes.3 {
                lower_generate_item(syntax_tree, item, path, module);
            }
        }
    }
}

fn lower_conditional_generate_construct(
    syntax_tree: &SyntaxTree,
    construct: &sv_parser::ConditionalGenerateConstruct,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match construct {
        sv_parser::ConditionalGenerateConstruct::If(construct) => {
            let cond = lower_constant_expression_to_expr(
                syntax_tree,
                &construct.nodes.1.nodes.1,
                module,
                path,
            )
            .and_then(|expr| {
                const_eval_param_expr(&expr, &module.parameters, "a generate `if` condition")
            });
            match cond {
                Ok(value) if value.truthy() => {
                    lower_generate_block(syntax_tree, &construct.nodes.2, path, module);
                }
                Ok(_) => {
                    if let Some((_, else_block)) = &construct.nodes.3 {
                        lower_generate_block(syntax_tree, else_block, path, module);
                    }
                }
                Err(diag) => module.unsupported.push(diag),
            }
        }
        sv_parser::ConditionalGenerateConstruct::Case(_) => {
            module.unsupported.push(Diagnostic {
                message: "generate case constructs are not supported yet".into(),
                span: module.span.clone(),
            });
        }
    }
}

fn lower_module_declaration_item(
    syntax_tree: &SyntaxTree,
    decl: &ModuleOrGenerateItemDeclaration,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match decl {
        ModuleOrGenerateItemDeclaration::PackageOrGenerateItemDeclaration(decl) => match &**decl {
            PackageOrGenerateItemDeclaration::DataDeclaration(decl) => {
                match lower_data_declaration(syntax_tree, decl, path, &module.parameters) {
                    Ok(decls) => {
                        module.signals.extend(decls.signals);
                        module.memories.extend(decls.memories);
                    }
                    Err(diag) => module.unsupported.push(diag),
                }
            }
            PackageOrGenerateItemDeclaration::NetDeclaration(decl) => {
                match lower_net_declaration(syntax_tree, decl, module, path) {
                    Ok(lowered) => {
                        module.signals.extend(lowered.signals);
                        module.continuous_assignments.extend(lowered.initializers);
                    }
                    Err(diag) => module.unsupported.push(diag),
                }
            }
            PackageOrGenerateItemDeclaration::LocalParameterDeclaration(decl) => {
                lower_parameter_or_localparam_declaration_into(
                    syntax_tree,
                    ParameterOrLocal::from_localparam_declaration(&decl.0),
                    path,
                    module,
                );
            }
            PackageOrGenerateItemDeclaration::ParameterDeclaration(decl) => {
                lower_parameter_or_localparam_declaration_into(
                    syntax_tree,
                    ParameterOrLocal::from_parameter_declaration(&decl.0),
                    path,
                    module,
                );
            }
            PackageOrGenerateItemDeclaration::TaskDeclaration(decl) => {
                if is_inert_task_declaration(syntax_tree, decl.as_ref()) {
                    return;
                }
                module.unsupported.push(Diagnostic {
                    message: "task declarations are not supported yet".into(),
                    span: module.span.clone(),
                });
            }
            PackageOrGenerateItemDeclaration::Empty(_) => {}
            _ => module.unsupported.push(Diagnostic {
                message: "declaration is outside the current executable subset".into(),
                span: module.span.clone(),
            }),
        },
        _ => module.unsupported.push(Diagnostic {
            message: "declaration is outside the current executable subset".into(),
            span: module.span.clone(),
        }),
    }
}

fn lower_ansi_port_declaration(
    syntax_tree: &SyntaxTree,
    decl: &AnsiPortDeclaration,
    path: &Path,
    inherited: Option<AnsiPortContext>,
    params: &[ParameterDecl],
) -> LowerResult<(PortDecl, AnsiPortContext)> {
    match decl {
        AnsiPortDeclaration::Net(decl) => {
            let context = if let Some(header) = decl.nodes.0.as_ref() {
                match header {
                    sv_parser::NetPortHeaderOrInterfacePortHeader::NetPortHeader(header) => {
                        let direction = lower_port_direction(header.nodes.0.as_ref(), path)?;
                        AnsiPortContext {
                            direction,
                            storage: lower_net_port_storage_kind(direction, &header.nodes.1)?,
                            range: lower_net_port_range(
                                syntax_tree,
                                &header.nodes.1,
                                path,
                                params,
                            )?,
                        }
                    }
                    sv_parser::NetPortHeaderOrInterfacePortHeader::InterfacePortHeader(_) => {
                        return Err(unsupported("interface ports are not supported yet", None));
                    }
                }
            } else {
                inherited
                    .ok_or_else(|| unsupported("ports must declare an explicit direction", None))?
            };
            if !decl.nodes.2.is_empty() || decl.nodes.3.is_some() {
                return Err(unsupported(
                    "ANSI ports with unpacked dimensions or default values are not supported yet",
                    None,
                ));
            }
            let (name, locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.1))
                    .ok_or_else(|| unsupported("failed to determine ANSI port name", None))?;
            Ok((
                PortDecl {
                    name,
                    direction: context.direction,
                    storage: context.storage,
                    range: context.range,
                    span: Some(span_from_locate(path, locate)),
                },
                context,
            ))
        }
        AnsiPortDeclaration::Variable(decl) => {
            let context = if let Some(header) = decl.nodes.0.as_ref() {
                AnsiPortContext {
                    direction: lower_port_direction(header.nodes.0.as_ref(), path)?,
                    storage: lower_variable_port_storage_kind(&header.nodes.1)?,
                    range: lower_variable_port_range(syntax_tree, &header.nodes.1, path, params)?,
                }
            } else {
                inherited
                    .ok_or_else(|| unsupported("ports must declare an explicit direction", None))?
            };
            if !decl.nodes.2.is_empty() || decl.nodes.3.is_some() {
                return Err(unsupported(
                    "ANSI ports with unpacked dimensions or default values are not supported yet",
                    None,
                ));
            }
            let (name, locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.1))
                    .ok_or_else(|| unsupported("failed to determine ANSI port name", None))?;
            Ok((
                PortDecl {
                    name,
                    direction: context.direction,
                    storage: context.storage,
                    range: context.range,
                    span: Some(span_from_locate(path, locate)),
                },
                context,
            ))
        }
        AnsiPortDeclaration::Paren(_) => Err(unsupported(
            "parenthesized ANSI ports are not supported yet",
            None,
        )),
    }
}

fn lower_port_direction(
    direction: Option<&PortDirection>,
    path: &Path,
) -> LowerResult<HirPortDirection> {
    match direction {
        Some(PortDirection::Input(_)) => Ok(HirPortDirection::Input),
        Some(PortDirection::Output(_)) => Ok(HirPortDirection::Output),
        Some(PortDirection::Inout(_)) => Ok(HirPortDirection::Inout),
        Some(PortDirection::Ref(_)) => Ok(HirPortDirection::Ref),
        None => Err(unsupported(
            format!(
                "port declaration in {} is missing a direction",
                path.display()
            ),
            None,
        )),
    }
}

fn lower_net_port_range(
    syntax_tree: &SyntaxTree,
    port_type: &sv_parser::NetPortType,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match port_type {
        sv_parser::NetPortType::DataType(data_type) => {
            lower_data_type_or_implicit_range(syntax_tree, &data_type.nodes.1, path, params)
        }
        _ => Err(unsupported("unsupported net port type", None)),
    }
}

fn lower_net_port_storage_kind(
    direction: HirPortDirection,
    port_type: &sv_parser::NetPortType,
) -> LowerResult<StorageKind> {
    match port_type {
        sv_parser::NetPortType::DataType(data_type) => {
            if let Some(net_type) = data_type.nodes.0.as_ref() {
                return Ok(StorageKind::Net(lower_net_kind(net_type)));
            }

            let storage = match direction {
                HirPortDirection::Input | HirPortDirection::Inout => {
                    StorageKind::Net(NetKind::Wire)
                }
                HirPortDirection::Output => match &data_type.nodes.1 {
                    DataTypeOrImplicit::ImplicitDataType(_) => StorageKind::Net(NetKind::Wire),
                    DataTypeOrImplicit::DataType(_) => StorageKind::Variable,
                },
                HirPortDirection::Ref => StorageKind::Variable,
            };
            Ok(storage)
        }
        _ => Err(unsupported("unsupported net port type", None)),
    }
}

fn lower_variable_port_range(
    syntax_tree: &SyntaxTree,
    port_type: &VariablePortType,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match &port_type.nodes.0 {
        sv_parser::VarDataType::DataType(data_type) => {
            lower_data_type_range(syntax_tree, data_type, path, params)
        }
        sv_parser::VarDataType::Var(var_type) => {
            lower_data_type_or_implicit_range(syntax_tree, &var_type.nodes.1, path, params)
        }
    }
}

fn lower_variable_port_storage_kind(port_type: &VariablePortType) -> LowerResult<StorageKind> {
    match &port_type.nodes.0 {
        sv_parser::VarDataType::DataType(_) | sv_parser::VarDataType::Var(_) => {
            Ok(StorageKind::Variable)
        }
    }
}

fn lower_data_declaration(
    syntax_tree: &SyntaxTree,
    decl: &DataDeclaration,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<LoweredDeclarations> {
    match decl {
        DataDeclaration::Variable(decl) => {
            let range =
                lower_data_type_or_implicit_range(syntax_tree, &decl.nodes.3, path, params)?;
            let mut lowered = LoweredDeclarations::default();
            for assignment in decl.nodes.4.nodes.0.contents() {
                let sv_parser::VariableDeclAssignment::Variable(assignment) = assignment else {
                    return Err(unsupported(
                        "complex variable declarations are not supported yet",
                        None,
                    ));
                };
                if assignment.nodes.2.is_some() {
                    return Err(unsupported(
                        "variable declarations with initializers are not supported yet",
                        None,
                    ));
                }
                let (name, locate) =
                    identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
                        .ok_or_else(|| {
                            unsupported("failed to determine variable declaration name", None)
                        })?;
                let span = Some(span_from_locate(path, locate));
                match lower_variable_dimensions(syntax_tree, &assignment.nodes.1, path, params)? {
                    None => lowered.signals.push(SignalDecl {
                        name,
                        storage: StorageKind::Variable,
                        range,
                        span,
                    }),
                    Some(index_range) => lowered.memories.push(MemoryDecl {
                        name,
                        storage: StorageKind::Variable,
                        element_range: range,
                        index_range,
                        span,
                    }),
                }
            }
            Ok(lowered)
        }
        _ => Err(unsupported("data declaration is not supported yet", None)),
    }
}

fn lower_net_declaration(
    syntax_tree: &SyntaxTree,
    decl: &NetDeclaration,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<LoweredNetDeclarations> {
    match decl {
        NetDeclaration::NetType(decl) => {
            let range = lower_data_type_or_implicit_range(
                syntax_tree,
                &decl.nodes.3,
                path,
                &module.parameters,
            )?;
            let mut lowered = LoweredNetDeclarations::default();
            for assignment in decl.nodes.5.nodes.0.contents() {
                if !assignment.nodes.1.is_empty() {
                    return Err(unsupported(
                        "net declarations with unpacked dimensions are not supported yet",
                        None,
                    ));
                }
                let (name, locate) =
                    identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
                        .ok_or_else(|| {
                            unsupported("failed to determine net declaration name", None)
                        })?;
                let signal = SignalDecl {
                    name,
                    storage: StorageKind::Net(lower_net_kind(&decl.nodes.0)),
                    range,
                    span: Some(span_from_locate(path, locate)),
                };
                if let Some((_, expr)) = assignment.nodes.2.as_ref() {
                    let mut expr_module = module.clone();
                    expr_module.signals.extend(lowered.signals.clone());
                    expr_module.signals.push(signal.clone());
                    lowered.initializers.push(HirContinuousAssign {
                        target: LValue::Signal(signal.name.clone()),
                        expr: lower_expression(syntax_tree, expr, &expr_module, path)?,
                        span: signal.span.clone(),
                    });
                }
                lowered.signals.push(signal);
            }
            Ok(lowered)
        }
        _ => Err(unsupported("net declaration is not supported yet", None)),
    }
}

fn lower_data_type_or_implicit_range(
    syntax_tree: &SyntaxTree,
    data_type: &DataTypeOrImplicit,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match data_type {
        DataTypeOrImplicit::DataType(data_type) => {
            lower_data_type_range(syntax_tree, data_type, path, params)
        }
        DataTypeOrImplicit::ImplicitDataType(data_type) => {
            lower_implicit_data_type_range(syntax_tree, data_type, path, params)
        }
    }
}

fn lower_data_type_range(
    syntax_tree: &SyntaxTree,
    data_type: &DataType,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match data_type {
        DataType::Vector(data_type) => {
            lower_packed_dimensions(syntax_tree, &data_type.nodes.2, path, params)
        }
        DataType::Atom(_) => Ok(None),
        DataType::Type(data_type) => {
            lower_packed_dimensions(syntax_tree, &data_type.nodes.2, path, params)
        }
        _ => Err(unsupported(
            "data type is outside the current executable subset",
            None,
        )),
    }
}

fn lower_net_kind(net_type: &sv_parser::NetType) -> NetKind {
    match net_type {
        sv_parser::NetType::Supply0(_) => NetKind::Supply0,
        sv_parser::NetType::Supply1(_) => NetKind::Supply1,
        sv_parser::NetType::Tri(_) => NetKind::Tri,
        sv_parser::NetType::Triand(_) => NetKind::Triand,
        sv_parser::NetType::Trior(_) => NetKind::Trior,
        sv_parser::NetType::Trireg(_) => NetKind::Trireg,
        sv_parser::NetType::Tri0(_) => NetKind::Tri0,
        sv_parser::NetType::Tri1(_) => NetKind::Tri1,
        sv_parser::NetType::Uwire(_) => NetKind::Uwire,
        sv_parser::NetType::Wire(_) => NetKind::Wire,
        sv_parser::NetType::Wand(_) => NetKind::Wand,
        sv_parser::NetType::Wor(_) => NetKind::Wor,
    }
}

fn lower_implicit_data_type_range(
    syntax_tree: &SyntaxTree,
    data_type: &ImplicitDataType,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    lower_packed_dimensions(syntax_tree, &data_type.nodes.1, path, params)
}

fn lower_unpacked_dimensions(
    syntax_tree: &SyntaxTree,
    unpacked_dimensions: &[UnpackedDimension],
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match unpacked_dimensions {
        [] => Ok(None),
        [UnpackedDimension::Range(range)] => lower_constant_range(
            syntax_tree,
            &range.nodes.0.nodes.1,
            path,
            params,
            "an unpacked dimension",
        )
        .map(Some),
        [UnpackedDimension::Expression(_)] => Err(unsupported(
            "unsized unpacked dimensions are not supported yet",
            None,
        )),
        _ => Err(unsupported(
            "multiple unpacked dimensions are not supported yet",
            None,
        )),
    }
}

fn lower_variable_dimensions(
    syntax_tree: &SyntaxTree,
    dimensions: &[VariableDimension],
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match dimensions {
        [] => Ok(None),
        [VariableDimension::UnpackedDimension(dimension)] => lower_unpacked_dimensions(
            syntax_tree,
            std::slice::from_ref(dimension.as_ref()),
            path,
            params,
        ),
        [VariableDimension::UnsizedDimension(_)]
        | [VariableDimension::AssociativeDimension(_)]
        | [VariableDimension::QueueDimension(_)] => Err(unsupported(
            "only fixed-size unpacked dimensions are supported today",
            None,
        )),
        _ => Err(unsupported(
            "multiple unpacked dimensions are not supported yet",
            None,
        )),
    }
}

fn lower_packed_dimensions(
    syntax_tree: &SyntaxTree,
    packed_dimensions: &[sv_parser::PackedDimension],
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match packed_dimensions {
        [] => Ok(None),
        [sv_parser::PackedDimension::Range(range)] => lower_constant_range(
            syntax_tree,
            &range.nodes.0.nodes.1,
            path,
            params,
            "a packed declaration range",
        )
        .map(Some),
        _ => Err(unsupported(
            "multiple packed dimensions are not supported yet",
            None,
        )),
    }
}

fn lower_continuous_assign(
    syntax_tree: &SyntaxTree,
    assign: &ContinuousAssign,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Vec<HirContinuousAssign>> {
    match assign {
        ContinuousAssign::Net(assign) => {
            let mut lowered = Vec::new();
            for assignment in assign.nodes.3.nodes.0.contents() {
                lowered.push(HirContinuousAssign {
                    target: lower_net_lvalue(syntax_tree, &assignment.nodes.0, path)?,
                    expr: lower_expression(syntax_tree, &assignment.nodes.2, module, path)?,
                    span: None,
                });
            }
            Ok(lowered)
        }
        ContinuousAssign::Variable(assign) => {
            let mut lowered = Vec::new();
            for assignment in assign.nodes.2.nodes.0.contents() {
                lowered.push(HirContinuousAssign {
                    target: lower_variable_assignment_lvalue(
                        syntax_tree,
                        assignment,
                        module,
                        path,
                    )?,
                    expr: lower_expression(syntax_tree, &assignment.nodes.2, module, path)?,
                    span: None,
                });
            }
            Ok(lowered)
        }
    }
}

fn lower_module_instantiation(
    syntax_tree: &SyntaxTree,
    instantiation: &ModuleInstantiation,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Vec<ModuleInstanceSummary>> {
    let (module_name, _) =
        identifier_name_from_node(syntax_tree, RefNode::from(&instantiation.nodes.0))
            .ok_or_else(|| unsupported("failed to determine instantiated module name", None))?;
    let mut parameter_overrides = Vec::new();
    if let Some(parameter_value_assignment) = &instantiation.nodes.1 {
        let Some(assignments) = parameter_value_assignment.nodes.1.nodes.1.as_ref() else {
            return Err(unsupported(
                "empty parameter override lists are not supported yet",
                None,
            ));
        };
        let ListOfParameterAssignments::Named(assignments) = assignments else {
            return Err(unsupported(
                "ordered parameter overrides are not supported yet",
                None,
            ));
        };

        for assignment in assignments.nodes.0.contents() {
            let (parameter_name, locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.1))
                    .ok_or_else(|| unsupported("failed to determine parameter name", None))?;
            let expr = assignment
                .nodes
                .2
                .nodes
                .1
                .as_ref()
                .ok_or_else(|| {
                    unsupported("named parameter overrides must provide an expression", None)
                })
                .and_then(|expr| lower_param_expression(syntax_tree, expr, module, path))?;
            parameter_overrides.push(HirNamedParameterAssign {
                parameter_name,
                expr,
                span: Some(span_from_locate(path, locate)),
            });
        }
    }
    let mut instances = Vec::new();

    for instance in instantiation.nodes.2.contents() {
        let (instance_name, locate) =
            identifier_name_from_node(syntax_tree, RefNode::from(&instance.nodes.0.nodes.0))
                .ok_or_else(|| unsupported("failed to determine instance name", None))?;
        let Some(port_connections) = instance.nodes.1.nodes.1.as_ref() else {
            return Err(unsupported(
                "module instantiations must use explicit named port connections",
                None,
            ));
        };
        let ListOfPortConnections::Named(connections) = port_connections else {
            return Err(unsupported(
                "ordered port connections are not supported yet",
                None,
            ));
        };

        let mut lowered_connections = Vec::new();
        for connection in connections.nodes.0.contents() {
            let NamedPortConnection::Identifier(connection) = connection else {
                return Err(unsupported(
                    "wildcard port connections are not supported yet",
                    None,
                ));
            };
            let (port_name, port_locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&connection.nodes.2))
                    .ok_or_else(|| unsupported("failed to determine connected port name", None))?;
            let expr = connection
                .nodes
                .3
                .as_ref()
                .and_then(|paren| paren.nodes.1.as_ref())
                .ok_or_else(|| {
                    unsupported("named port connections must provide an expression", None)
                })
                .and_then(|expr| lower_expression(syntax_tree, expr, module, path))?;
            lowered_connections.push(HirNamedPortConnection {
                port_name,
                expr,
                span: Some(span_from_locate(path, port_locate)),
            });
        }

        instances.push(ModuleInstanceSummary {
            module_name: module_name.clone(),
            instance_name,
            span: Some(span_from_locate(path, locate)),
            parameter_overrides: parameter_overrides.clone(),
            connections: lowered_connections,
        });
    }

    Ok(instances)
}

fn lower_param_expression(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::ParamExpression,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        sv_parser::ParamExpression::MintypmaxExpression(expr) => {
            lower_mintypmax_expression_to_expr(syntax_tree, expr, module, path)
        }
        _ => Err(unsupported(
            "parameter override expression is outside the supported subset",
            None,
        )),
    }
}

fn lower_mintypmax_expression_to_expr(
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

fn lower_always_construct(
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
fn lower_always_generic(
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

fn lower_always_ff_statement(
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

fn lower_always_ff_event_control(
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

fn lower_always_ff_event_expression(
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

fn collect_always_ff_event_signals(
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

fn looks_like_reset_signal(name: &str) -> bool {
    matches!(name, "reset" | "rst" | "resetn" | "rst_n" | "rstn")
        || name.starts_with("reset_")
        || name.starts_with("rst_")
}

fn lower_statement(
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

fn lower_statement_or_null(
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

fn lower_loop_statement(
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
            None,
        )),
    }
}

fn lower_for_loop_statement(
    syntax_tree: &SyntaxTree,
    statement: &sv_parser::LoopStatementFor,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Stmt> {
    let controls = &statement.nodes.1.nodes.1;
    let Some(init) = controls.0.as_ref() else {
        return Err(unsupported(
            "procedural `for` loops require an initialization assignment",
            None,
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
        None,
    ))
}

fn lower_for_loop_initialization(
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
                    None,
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
                    None,
                ));
            };
            let assignments = declaration.nodes.2.contents();
            let [(identifier, _, expr)] = assignments.as_slice() else {
                return Err(unsupported(
                    "procedural `for` loops only support a single initialized loop variable",
                    None,
                ));
            };
            let (name, _) = identifier_name_from_node(syntax_tree, RefNode::from(identifier))
                .ok_or_else(|| unsupported("failed to determine loop variable name", None))?;
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

fn lower_for_loop_variable_assignment(
    syntax_tree: &SyntaxTree,
    assignment: &VariableAssignment,
    module: &ModuleSummary,
    path: &Path,
    context: &str,
) -> LowerResult<(String, Value)> {
    if symbol_text(syntax_tree, &assignment.nodes.1)? != "=" {
        return Err(unsupported(format!("{context} must use `=`"), None));
    }

    let LValue::Signal(name) =
        lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)?
    else {
        return Err(unsupported(
            format!("{context} must target a plain loop variable"),
            None,
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

fn lower_for_loop_step(
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
            None,
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
                    None,
                ));
            };
            if name != loop_var {
                return Err(unsupported(
                    "procedural `for` loop step must update the initialized loop variable",
                    None,
                ));
            }
            let loop_bits = loop_value.to_bit_value_checked().ok_or_else(|| {
                unsupported("procedural `for` loops require two-state loop values", None)
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

fn lower_for_loop_operator_step(
    syntax_tree: &SyntaxTree,
    assignment: &sv_parser::OperatorAssignment,
    module: &ModuleSummary,
    path: &Path,
    loop_var: &str,
) -> LowerResult<Value> {
    if symbol_text(syntax_tree, &assignment.nodes.1.nodes.0)? != "=" {
        return Err(unsupported("procedural `for` loop step must use `=`", None));
    }

    let LValue::Signal(name) =
        lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)?
    else {
        return Err(unsupported(
            "procedural `for` loop step must target a plain loop variable",
            None,
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

fn lower_const_eval_expression(
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
            None,
        )
    })
}

fn module_with_const_binding(module: &ModuleSummary, name: &str, value: &Value) -> ModuleSummary {
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

fn fold_loop_statements(statements: Vec<Stmt>) -> Stmt {
    if statements.is_empty() {
        Stmt::Empty
    } else {
        Stmt::Block(statements)
    }
}

fn normalize_for_loop_value(value: Value) -> Value {
    value.coerced_to(value.width.max(32))
}

fn substitute_stmt_ident(stmt: &Stmt, name: &str, replacement: &Expr) -> Stmt {
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

fn substitute_lvalue_ident(lvalue: &LValue, name: &str, replacement: &Expr) -> LValue {
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

fn substitute_expr_ident(expr: &Expr, name: &str, replacement: &Expr) -> Expr {
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

fn expr_from_const_eval_value(value: &Value) -> Expr {
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

fn lower_subroutine_call_statement(
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

fn is_inert_subroutine_call(syntax_tree: &SyntaxTree, call: &sv_parser::SubroutineCall) -> bool {
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

fn inert_task_name(
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

fn inert_system_tf_name(
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

fn is_inert_task_declaration(_syntax_tree: &SyntaxTree, decl: &sv_parser::TaskDeclaration) -> bool {
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

fn lower_blocking_assignment(
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

fn lower_nonblocking_assignment(
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

fn lower_seq_block(
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

fn lower_block_item_declaration_stmt(
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

fn lower_conditional_statement(
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

fn lower_conditional_else_chain(
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

fn lower_case_statement(
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

fn lower_case_item(
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

fn lower_expression(
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

fn rebalance_logical_rhs_binary(left: Expr, op: BinaryOp, right: Expr) -> Expr {
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

fn lower_primary(
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

fn lower_function_subroutine_call(
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

fn lower_concatenation(
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

fn lower_multiple_concatenation(
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

fn lower_mintypmax_expression(
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

fn lower_literal(
    syntax_tree: &SyntaxTree,
    literal: &sv_parser::PrimaryLiteral,
) -> LowerResult<Expr> {
    match literal {
        sv_parser::PrimaryLiteral::Number(number) => {
            Ok(Expr::Literal(lower_number(syntax_tree, number)?))
        }
        sv_parser::PrimaryLiteral::UnbasedUnsizedLiteral(literal) => {
            let text = symbol_text(syntax_tree, &literal.nodes.0)?;
            let bits = match text.as_str() {
                "'0" => LogicBits::from_bit_value(BitValue::zero()),
                "'1" => LogicBits::from_bit_value(BitValue::one()),
                "'x" | "'X" => LogicBits::filled(1, LogicBit::X),
                "'z" | "'Z" => LogicBits::filled(1, LogicBit::Z),
                _ => return Err(unsupported("unsupported unbased unsized literal", None)),
            };
            Ok(Expr::Literal(NumericLiteral { bits, width: None }))
        }
        sv_parser::PrimaryLiteral::StringLiteral(literal) => {
            let raw = syntax_tree
                .get_str(&literal.nodes.0)
                .ok_or_else(|| unsupported("failed to read string literal text", None))?;
            let bytes = parse_string_literal_bytes(raw)?;
            let width = (bytes.len() * 8).max(1);
            let mut bits = BitValue::zero();
            for byte in bytes {
                bits = bits.shift_left(8);
                bits = bits.bitor(&BitValue::from(byte as u64));
            }
            Ok(Expr::Literal(NumericLiteral {
                bits: LogicBits::from_bit_value(bits),
                width: Some(width),
            }))
        }
        _ => Err(unsupported(
            "literal is outside the current executable subset",
            None,
        )),
    }
}

fn lower_number(
    syntax_tree: &SyntaxTree,
    number: &sv_parser::Number,
) -> LowerResult<NumericLiteral> {
    let sv_parser::Number::IntegralNumber(number) = number else {
        return Err(unsupported("real numbers are not supported", None));
    };
    match &**number {
        sv_parser::IntegralNumber::DecimalNumber(number) => match &**number {
            sv_parser::DecimalNumber::UnsignedNumber(number) => {
                let text = syntax_tree
                    .get_str(&number.nodes.0)
                    .ok_or_else(|| unsupported("failed to read numeric literal text", None))?;
                let bits = BitValue::from_str_radix(&text.replace('_', ""), 10)
                    .map_err(|_| unsupported("failed to parse numeric literal", None))?;
                Ok(NumericLiteral {
                    bits: LogicBits::from_bit_value(bits),
                    width: Some(32),
                })
            }
            sv_parser::DecimalNumber::BaseUnsigned(number) => {
                let width = number
                    .nodes
                    .0
                    .as_ref()
                    .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                    .transpose()?;
                Ok(NumericLiteral {
                    bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 10, width)?,
                    width,
                })
            }
            _ => Err(unsupported("x/z decimal literals are not supported", None)),
        },
        sv_parser::IntegralNumber::BinaryNumber(number) => {
            let width = number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?;
            Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 2, width)?,
                width,
            })
        }
        sv_parser::IntegralNumber::OctalNumber(number) => {
            let width = number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?;
            Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 8, width)?,
                width,
            })
        }
        sv_parser::IntegralNumber::HexNumber(number) => {
            let width = number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?;
            Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 16, width)?,
                width,
            })
        }
    }
}

fn lower_expr_select(
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

fn lower_net_lvalue(
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

fn lower_variable_assignment_lvalue(
    syntax_tree: &SyntaxTree,
    assignment: &VariableAssignment,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<LValue> {
    lower_variable_lvalue(syntax_tree, &assignment.nodes.0, module, path)
}

fn lower_variable_lvalue(
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

fn lower_constant_select_lvalue(
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

fn lower_select_lvalue(
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

fn lower_hierarchical_identifier(
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

fn lower_net_identifier(
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

fn lower_cond_predicate(
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

fn lower_unary_operator(
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

fn lower_binary_operator(
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

fn lower_constant_range(
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

fn lower_constant_part_select_range(
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

fn lower_part_select_range(
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

fn lower_usize_expression(
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
            None,
        )),
    }
}

fn lower_usize_constant_expression(
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

fn lower_usize_constant_expression_with_params(
    syntax_tree: &SyntaxTree,
    expr: &ConstantExpression,
    path: &Path,
    params: &[ParameterDecl],
    frozen_construct: &str,
) -> LowerResult<usize> {
    let module = const_eval_module(params);
    let lowered = lower_constant_expression_to_expr(syntax_tree, expr, &module, path)?;
    const_eval_param_expr(&lowered, params, frozen_construct)?
        .to_usize_checked()
        .ok_or_else(|| unsupported("constant index exceeds host limits", None))
}

fn const_eval_module(params: &[ParameterDecl]) -> ModuleSummary {
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
struct FrozenParamRecording;

impl FrozenParamRecording {
    fn begin() -> Self {
        FROZEN_PARAM_RECORDER.with(|recorder| *recorder.borrow_mut() = Some(BTreeMap::new()));
        FrozenParamRecording
    }

    fn finish(self) -> BTreeMap<String, String> {
        FROZEN_PARAM_RECORDER
            .with(|recorder| recorder.borrow_mut().take())
            .unwrap_or_default()
    }
}

fn record_frozen_params(expr: &Expr, params: &[ParameterDecl], frozen_construct: &str) {
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

fn collect_expr_param_refs(expr: &Expr, params: &[ParameterDecl], names: &mut Vec<String>) {
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
fn const_eval_param_expr(
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

fn const_eval_param_value(
    expr: &Expr,
    params: &[ParameterDecl],
    frozen_construct: &str,
) -> LowerResult<usize> {
    const_eval_param_expr(expr, params, frozen_construct)?
        .to_usize_checked()
        .ok_or_else(|| unsupported("constant value exceeds host limits", None))
}

fn parse_based_value(
    syntax_tree: &SyntaxTree,
    locate: &Locate,
    radix: u32,
    explicit_width: Option<usize>,
) -> LowerResult<LogicBits> {
    let text = syntax_tree
        .get_str(locate)
        .ok_or_else(|| unsupported("failed to read numeric literal text", None))?;
    let cleaned: String = text.chars().filter(|ch| *ch != '_').collect();
    if cleaned.is_empty() {
        return Err(unsupported("numeric literal has no digits", None));
    }

    let bits_per_digit = match radix {
        2 => 1,
        8 => 3,
        16 => 4,
        10 => return parse_decimal_logic_bits(&cleaned, explicit_width),
        _ => return Err(unsupported("unsupported numeric literal radix", None)),
    };

    let natural_width = cleaned.chars().count() * bits_per_digit;
    let mut bits = LogicBits::zero();
    for (digit_index, ch) in cleaned.chars().rev().enumerate() {
        let base = digit_index * bits_per_digit;
        match ch {
            'x' | 'X' => {
                for offset in 0..bits_per_digit {
                    bits.set_bit(base + offset, LogicBit::X);
                }
            }
            'z' | 'Z' | '?' => {
                for offset in 0..bits_per_digit {
                    bits.set_bit(base + offset, LogicBit::Z);
                }
            }
            other => {
                let digit = other.to_digit(radix).ok_or_else(|| {
                    unsupported(
                        format!("invalid digit '{}' in numeric literal", other),
                        None,
                    )
                })?;
                for offset in 0..bits_per_digit {
                    if (digit >> offset) & 1 == 1 {
                        bits.set_bit(base + offset, LogicBit::One);
                    }
                }
            }
        }
    }

    if let Some(target) = explicit_width {
        if target > natural_width && natural_width > 0 {
            let top = bits.bit(natural_width - 1);
            if matches!(top, LogicBit::X | LogicBit::Z) {
                for index in natural_width..target {
                    bits.set_bit(index, top);
                }
            }
        }
        Ok(bits.truncate(target))
    } else {
        Ok(bits)
    }
}

fn parse_decimal_logic_bits(
    cleaned: &str,
    explicit_width: Option<usize>,
) -> LowerResult<LogicBits> {
    for ch in cleaned.chars() {
        if matches!(ch, 'x' | 'X' | 'z' | 'Z' | '?') {
            return Err(unsupported(
                "x/z digits are not supported in decimal literals",
                None,
            ));
        }
    }
    let bits = BitValue::from_str_radix(cleaned, 10)
        .map_err(|_| unsupported("failed to parse numeric literal", None))?;
    let bits = LogicBits::from_bit_value(bits);
    Ok(match explicit_width {
        Some(target) => bits.truncate(target),
        None => bits,
    })
}

fn parse_string_literal_bytes(text: &str) -> LowerResult<Vec<u8>> {
    if !(text.starts_with('"') && text.ends_with('"')) {
        return Err(unsupported("string literal is malformed", None));
    }

    let mut bytes = Vec::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if !ch.is_ascii() {
                return Err(unsupported(
                    "non-ASCII string literals are not supported yet",
                    None,
                ));
            }
            bytes.push(ch as u8);
            continue;
        }

        let escaped = chars
            .next()
            .ok_or_else(|| unsupported("string literal ends with a dangling escape", None))?;
        let byte = match escaped {
            'n' => b'\n',
            'r' => b'\r',
            't' => b'\t',
            '\\' => b'\\',
            '"' => b'"',
            '0' => b'\0',
            other if other.is_ascii() => other as u8,
            _ => {
                return Err(unsupported(
                    "non-ASCII string literals are not supported yet",
                    None,
                ));
            }
        };
        bytes.push(byte);
    }

    Ok(bytes)
}

fn locate_usize(syntax_tree: &SyntaxTree, locate: &Locate) -> LowerResult<usize> {
    syntax_tree
        .get_str(locate)
        .ok_or_else(|| unsupported("failed to read integer text", None))?
        .replace('_', "")
        .parse::<usize>()
        .map_err(|_| unsupported("failed to parse integer", None))
}

fn span_from_locate(path: &Path, locate: Locate) -> SourceSpan {
    SourceSpan {
        path: path.to_path_buf(),
        line: locate.line as usize,
        column: 1,
    }
}

fn symbol_text(syntax_tree: &SyntaxTree, symbol: &sv_parser::Symbol) -> LowerResult<String> {
    syntax_tree
        .get_str(&symbol.nodes.0)
        .map(str::to_owned)
        .ok_or_else(|| unsupported("failed to read symbol text", None))
}

fn unsupported(message: impl Into<String>, span: Option<SourceSpan>) -> Diagnostic {
    Diagnostic {
        message: message.into(),
        span,
    }
}

fn identifier_name_from_node(
    syntax_tree: &SyntaxTree,
    node: RefNode<'_>,
) -> Option<(String, Locate)> {
    let locate = get_identifier(node)?;
    let name = syntax_tree.get_str(&locate)?.to_owned();
    Some((name, locate))
}

fn get_identifier(node: RefNode) -> Option<Locate> {
    match unwrap_node!(node, SimpleIdentifier, EscapedIdentifier) {
        Some(RefNode::SimpleIdentifier(identifier)) => Some(identifier.nodes.0),
        Some(RefNode::EscapedIdentifier(identifier)) => Some(identifier.nodes.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
