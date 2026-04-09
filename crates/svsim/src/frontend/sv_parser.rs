use std::collections::HashMap;
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
use crate::hir::{
    AssignmentKind, BinaryOp, CaseStmtItem, ContinuousAssign as HirContinuousAssign, Expr, LValue,
    MemoryDecl, ModuleDeclStyle, ModuleInstanceSummary, ModuleSummary,
    NamedParameterAssign as HirNamedParameterAssign, NamedPortConnection as HirNamedPortConnection,
    NetKind, NumericLiteral, PackedRange, ParameterDecl, PortDecl,
    PortDirection as HirPortDirection, ProcBlock, ProcBlockKind, SignalDecl, SourceFile, Stmt,
    StorageKind, UnaryOp,
};
use crate::width::{
    arithmetic_shift_right_bits, mask, minimum_width, shift_left_bits, shift_right_bits,
    sign_extend_bits,
};

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
    };

    for item in &decl.nodes.2 {
        if let sv_parser::ModuleItem::NonPortModuleItem(item) = item {
            lower_non_port_module_item(syntax_tree, item, path, &mut module);
        }
    }

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
                .to_usize_checked()
                .ok_or_else(|| unsupported("replication count exceeds host limits", None))?;
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
            .and_then(|expr| const_eval_param_expr(&expr, &module.parameters));
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
        [UnpackedDimension::Range(range)] => {
            lower_constant_range(syntax_tree, &range.nodes.0.nodes.1, path, params).map(Some)
        }
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
        [sv_parser::PackedDimension::Range(range)] => {
            lower_constant_range(syntax_tree, &range.nodes.0.nodes.1, path, params).map(Some)
        }
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
        if !const_eval_param_expr(&cond, &iteration_module.parameters)?.truthy() {
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
) -> LowerResult<(String, ConstEvalValue)> {
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
) -> LowerResult<(String, ConstEvalValue)> {
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
    loop_value: &ConstEvalValue,
) -> LowerResult<ConstEvalValue> {
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
            match symbol_text(syntax_tree, &op.nodes.0)?.as_str() {
                "++" => Ok(normalize_for_loop_value(ConstEvalValue::new_with_signed(
                    loop_value
                        .normalized_bits()
                        .wrapping_add(&BitValue::from(1u64), loop_value.width),
                    loop_value.width,
                    loop_value.signed,
                ))),
                "--" => Ok(normalize_for_loop_value(ConstEvalValue::new_with_signed(
                    loop_value
                        .normalized_bits()
                        .wrapping_sub(&BitValue::from(1u64), loop_value.width),
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
) -> LowerResult<ConstEvalValue> {
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
) -> LowerResult<ConstEvalValue> {
    let lowered = lower_expression(syntax_tree, expr, module, path)?;
    const_eval_param_expr(&lowered, &module.parameters).map_err(|_| {
        unsupported(
            "procedural `for` loops require constant-bounded expressions",
            None,
        )
    })
}

fn module_with_const_binding(
    module: &ModuleSummary,
    name: &str,
    value: &ConstEvalValue,
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

fn fold_loop_statements(statements: Vec<Stmt>) -> Stmt {
    if statements.is_empty() {
        Stmt::Empty
    } else {
        Stmt::Block(statements)
    }
}

fn normalize_for_loop_value(value: ConstEvalValue) -> ConstEvalValue {
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

fn expr_from_const_eval_value(value: &ConstEvalValue) -> Expr {
    let literal = Expr::Literal(NumericLiteral {
        bits: value.normalized_bits(),
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
    if let Ok(value) = const_eval_param_expr(&cond, &module.parameters) {
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
    if let Ok(value) = const_eval_param_expr(&cond, &module.parameters) {
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
        count: lower_usize_expression(syntax_tree, &concat.nodes.0.nodes.1.0, module, path)?,
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
                "'0" => BitValue::zero(),
                "'1" => BitValue::one(),
                "'x" | "'X" | "'z" | "'Z" => BitValue::zero(),
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
                bits,
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
                    bits,
                    width: Some(32),
                })
            }
            sv_parser::DecimalNumber::BaseUnsigned(number) => Ok(NumericLiteral {
                bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 10)?,
                width: number
                    .nodes
                    .0
                    .as_ref()
                    .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                    .transpose()?,
            }),
            _ => Err(unsupported("x/z decimal literals are not supported", None)),
        },
        sv_parser::IntegralNumber::BinaryNumber(number) => Ok(NumericLiteral {
            bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 2)?,
            width: number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?,
        }),
        sv_parser::IntegralNumber::OctalNumber(number) => Ok(NumericLiteral {
            bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 8)?,
            width: number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?,
        }),
        sv_parser::IntegralNumber::HexNumber(number) => Ok(NumericLiteral {
            bits: parse_based_value(syntax_tree, &number.nodes.2.nodes.0, 16)?,
            width: number
                .nodes
                .0
                .as_ref()
                .map(|size| locate_usize(syntax_tree, &size.nodes.0.nodes.0))
                .transpose()?,
        }),
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
                index: lower_usize_expression(syntax_tree, &index.nodes.1, module, path)?,
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
                index: lower_usize_expression(syntax_tree, &index.nodes.1, module, path)?,
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
) -> LowerResult<PackedRange> {
    Ok(PackedRange {
        msb: lower_usize_constant_expression_with_params(
            syntax_tree,
            &range.nodes.0,
            path,
            params,
        )?,
        lsb: lower_usize_constant_expression_with_params(
            syntax_tree,
            &range.nodes.2,
            path,
            params,
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
            let base = lower_usize_expression(syntax_tree, &range.nodes.0, module, path)?;
            let width = lower_usize_constant_expression_with_params(
                syntax_tree,
                &range.nodes.2,
                path,
                &module.parameters,
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
) -> LowerResult<usize> {
    let lowered = lower_expression(syntax_tree, expr, module, path)?;
    match const_eval_param_value(&lowered, &module.parameters) {
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
    lower_usize_constant_expression_with_params(syntax_tree, expr, path, &[])
}

fn lower_usize_constant_expression_with_params(
    syntax_tree: &SyntaxTree,
    expr: &ConstantExpression,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<usize> {
    let module = const_eval_module(params);
    let lowered = lower_constant_expression_to_expr(syntax_tree, expr, &module, path)?;
    const_eval_param_expr(&lowered, params)?
        .to_usize_checked()
        .ok_or_else(|| unsupported("constant index exceeds host limits", None))
}

#[derive(Debug, Clone)]
struct ConstEvalValue {
    bits: BitValue,
    width: usize,
    signed: bool,
}

impl ConstEvalValue {
    fn new(bits: BitValue, width: usize) -> Self {
        Self::new_with_signed(bits, width, false)
    }

    fn new_with_signed(bits: BitValue, width: usize, signed: bool) -> Self {
        Self {
            bits: bits.truncate(width),
            width,
            signed,
        }
    }

    fn normalized_bits(&self) -> BitValue {
        self.bits.clone()
    }

    fn coerced_to(&self, width: usize) -> Self {
        let width = width.max(1);
        let bits = if self.signed {
            sign_extend_bits(&self.normalized_bits(), self.width, width)
        } else {
            self.normalized_bits().truncate(width)
        };
        Self::new_with_signed(bits, width, self.signed)
    }

    fn truthy(&self) -> bool {
        !self.bits.is_zero()
    }

    fn to_usize_checked(&self) -> Option<usize> {
        let bits = self.normalized_bits();
        if self.signed && self.width.max(1) > 0 && bits.get_bit(self.width.max(1) - 1) {
            return None;
        }
        bits.to_usize_checked()
    }
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
    }
}

fn const_eval_param_expr(expr: &Expr, params: &[ParameterDecl]) -> LowerResult<ConstEvalValue> {
    match expr {
        Expr::Ident(name) => {
            let param = params.iter().find(|p| p.name == *name).ok_or_else(|| {
                unsupported(
                    format!("parameter '{name}' not found for constant evaluation"),
                    None,
                )
            })?;
            const_eval_param_expr(&param.default_value, params)
        }
        Expr::Literal(literal) => Ok(const_eval_value_from_literal(literal)),
        Expr::Concat(exprs) => {
            let mut parts = Vec::with_capacity(exprs.len());
            for expr in exprs {
                parts.push(const_eval_param_expr(expr, params)?);
            }
            concat_const_eval_values(&parts)
        }
        Expr::Repeat { count, expr } => {
            let value = const_eval_param_expr(expr, params)?;
            let values = vec![value; *count];
            concat_const_eval_values(&values)
        }
        Expr::BitSelect { expr, index } => {
            let value = const_eval_param_expr(expr, params)?;
            if *index >= value.width {
                return Err(unsupported(
                    "bit select is out of range in constant expression",
                    None,
                ));
            }
            Ok(ConstEvalValue::new(
                BitValue::from(value.normalized_bits().get_bit(*index)),
                1,
            ))
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let value = const_eval_param_expr(expr, params)?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            if high >= value.width {
                return Err(unsupported(
                    "part select is out of range in constant expression",
                    None,
                ));
            }
            let width = high - low + 1;
            Ok(ConstEvalValue::new(
                value.normalized_bits().slice(low, width),
                width,
            ))
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            let cond = const_eval_param_expr(cond, params)?;
            let when_true = const_eval_param_expr(when_true, params)?;
            let when_false = const_eval_param_expr(when_false, params)?;
            let result_width = when_true.width.max(when_false.width);
            if cond.truthy() {
                Ok(when_true.coerced_to(result_width))
            } else {
                Ok(when_false.coerced_to(result_width))
            }
        }
        Expr::Unary { op, expr } => {
            let value = const_eval_param_expr(expr, params)?;
            match op {
                UnaryOp::BitNot => Ok(ConstEvalValue::new_with_signed(
                    value.normalized_bits().bitnot_with_width(value.width),
                    value.width,
                    value.signed,
                )),
                UnaryOp::Negate => Ok(ConstEvalValue::new_with_signed(
                    BitValue::zero().wrapping_sub(&value.normalized_bits(), value.width),
                    value.width,
                    value.signed,
                )),
                UnaryOp::LogicalNot => Ok(ConstEvalValue::new(
                    BitValue::from(u64::from(!value.truthy())),
                    1,
                )),
                UnaryOp::ReductionOr => Ok(ConstEvalValue::new(
                    BitValue::from(u64::from(value.truthy())),
                    1,
                )),
                UnaryOp::ReductionAnd => {
                    let all_ones = mask(value.width);
                    Ok(ConstEvalValue::new(
                        BitValue::from(u64::from(
                            value.normalized_bits().bitand(&all_ones) == all_ones,
                        )),
                        1,
                    ))
                }
                UnaryOp::ReductionNand => {
                    let all_ones = mask(value.width);
                    Ok(ConstEvalValue::new(
                        BitValue::from(u64::from(
                            value.normalized_bits().bitand(&all_ones) != all_ones,
                        )),
                        1,
                    ))
                }
                UnaryOp::ReductionXor => {
                    let mut count = 0u32;
                    let bits = value.normalized_bits();
                    for index in 0..value.width {
                        if !bits.slice(index, 1).is_zero() {
                            count += 1;
                        }
                    }
                    Ok(ConstEvalValue::new(
                        BitValue::from(u64::from(count % 2 != 0)),
                        1,
                    ))
                }
                UnaryOp::Signed => Ok(ConstEvalValue::new_with_signed(
                    value.normalized_bits(),
                    value.width,
                    true,
                )),
                UnaryOp::Unsigned => Ok(ConstEvalValue::new_with_signed(
                    value.normalized_bits(),
                    value.width,
                    false,
                )),
            }
        }
        Expr::Binary { left, op, right } => {
            if matches!(op, BinaryOp::LogicalAnd) {
                let left = const_eval_param_expr(left, params)?;
                if !left.truthy() {
                    return Ok(ConstEvalValue::new(BitValue::zero(), 1));
                }
                let right = const_eval_param_expr(right, params)?;
                return Ok(ConstEvalValue::new(
                    BitValue::from(u64::from(right.truthy())),
                    1,
                ));
            }
            if matches!(op, BinaryOp::LogicalOr) {
                let left = const_eval_param_expr(left, params)?;
                if left.truthy() {
                    return Ok(ConstEvalValue::new(BitValue::from(1_u64), 1));
                }
                let right = const_eval_param_expr(right, params)?;
                return Ok(ConstEvalValue::new(
                    BitValue::from(u64::from(right.truthy())),
                    1,
                ));
            }
            let mut left = const_eval_param_expr(left, params)?;
            let mut right = const_eval_param_expr(right, params)?;
            let common_width = left.width.max(right.width);
            left = left.coerced_to(common_width);
            right = right.coerced_to(common_width);
            let (bits, width, signed) = match op {
                BinaryOp::BitAnd => (
                    left.normalized_bits().bitand(&right.normalized_bits()),
                    common_width,
                    left.signed && right.signed,
                ),
                BinaryOp::BitOr => (
                    left.normalized_bits().bitor(&right.normalized_bits()),
                    common_width,
                    left.signed && right.signed,
                ),
                BinaryOp::BitXor => (
                    left.normalized_bits().bitxor(&right.normalized_bits()),
                    common_width,
                    left.signed && right.signed,
                ),
                BinaryOp::ShiftLeft => (
                    shift_left_bits(
                        &left.normalized_bits(),
                        &right.normalized_bits(),
                        left.width,
                    ),
                    left.width,
                    left.signed,
                ),
                BinaryOp::ShiftRight => (
                    shift_right_bits(
                        &left.normalized_bits(),
                        &right.normalized_bits(),
                        left.width,
                    ),
                    left.width,
                    left.signed,
                ),
                BinaryOp::ArithmeticShiftRight => (
                    arithmetic_shift_right_bits(
                        &left.normalized_bits(),
                        &right.normalized_bits(),
                        left.width,
                    ),
                    left.width,
                    left.signed,
                ),
                BinaryOp::LogicalAnd => unreachable!("handled before common-width coercion"),
                BinaryOp::LogicalOr => unreachable!("handled before common-width coercion"),
                BinaryOp::Eq => (
                    BitValue::from(left.normalized_bits() == right.normalized_bits()),
                    1,
                    false,
                ),
                BinaryOp::NotEq => (
                    BitValue::from(left.normalized_bits() != right.normalized_bits()),
                    1,
                    false,
                ),
                BinaryOp::Lt => (
                    BitValue::from(compare_const_eval_values(&left, &right).is_lt()),
                    1,
                    false,
                ),
                BinaryOp::LtEq => (
                    BitValue::from(!compare_const_eval_values(&left, &right).is_gt()),
                    1,
                    false,
                ),
                BinaryOp::Gt => (
                    BitValue::from(compare_const_eval_values(&left, &right).is_gt()),
                    1,
                    false,
                ),
                BinaryOp::GtEq => (
                    BitValue::from(!compare_const_eval_values(&left, &right).is_lt()),
                    1,
                    false,
                ),
                BinaryOp::Add => (
                    left.normalized_bits()
                        .wrapping_add(&right.normalized_bits(), common_width),
                    common_width,
                    left.signed && right.signed,
                ),
                BinaryOp::Sub => (
                    left.normalized_bits()
                        .wrapping_sub(&right.normalized_bits(), common_width),
                    common_width,
                    left.signed && right.signed,
                ),
                BinaryOp::Mul => (
                    left.normalized_bits()
                        .wrapping_mul(&right.normalized_bits(), common_width),
                    common_width,
                    left.signed && right.signed,
                ),
            };
            Ok(ConstEvalValue::new_with_signed(bits, width, signed))
        }
        Expr::MemoryRead { .. } => Err(unsupported(
            "expression is too complex for constant parameter evaluation",
            None,
        )),
    }
}

/// Evaluate an already-lowered HIR Expr to a usize, resolving parameter references.
fn const_eval_param_value(expr: &Expr, params: &[ParameterDecl]) -> LowerResult<usize> {
    const_eval_param_expr(expr, params)?
        .to_usize_checked()
        .ok_or_else(|| unsupported("constant value exceeds host limits", None))
}

fn const_eval_value_from_literal(literal: &NumericLiteral) -> ConstEvalValue {
    let width = literal
        .width
        .unwrap_or_else(|| minimum_width(&literal.bits));
    ConstEvalValue::new(literal.bits.clone(), width)
}

fn concat_const_eval_values(parts: &[ConstEvalValue]) -> LowerResult<ConstEvalValue> {
    let mut total_width = 0usize;
    for part in parts {
        total_width = total_width
            .checked_add(part.width)
            .ok_or_else(|| unsupported("constant value exceeds host limits", None))?;
    }
    if total_width == 0 {
        return Err(unsupported(
            "expression is too complex for constant parameter evaluation",
            None,
        ));
    }

    let mut bits = BitValue::zero();
    let mut shift = total_width;
    for part in parts {
        shift -= part.width;
        bits = bits.bitor(&part.normalized_bits().shift_left(shift));
    }

    Ok(ConstEvalValue::new(bits, total_width))
}

fn compare_const_eval_values(left: &ConstEvalValue, right: &ConstEvalValue) -> std::cmp::Ordering {
    if left.signed && right.signed {
        compare_signed_const_eval_bits(
            &left.normalized_bits(),
            &right.normalized_bits(),
            left.width,
        )
    } else {
        left.normalized_bits()
            .cmp_unsigned(&right.normalized_bits())
    }
}

fn compare_signed_const_eval_bits(
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

fn parse_based_value(
    syntax_tree: &SyntaxTree,
    locate: &Locate,
    radix: u32,
) -> LowerResult<BitValue> {
    let text = syntax_tree
        .get_str(locate)
        .ok_or_else(|| unsupported("failed to read numeric literal text", None))?;
    let cleaned = coerce_unknown_digits_to_zero(&text.replace('_', ""));
    BitValue::from_str_radix(&cleaned, radix)
        .map_err(|_| unsupported("failed to parse numeric literal", None))
}

fn coerce_unknown_digits_to_zero(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'x' | 'X' | 'z' | 'Z' | '?' => '0',
            other => other,
        })
        .collect()
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
mod tests {
    use std::path::PathBuf;

    use super::SvParserFrontend;
    use crate::hir::{
        AssignmentKind, BinaryOp, Expr, LValue, NetKind, NumericLiteral, ProcBlockKind, Stmt,
        StorageKind,
    };

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn parse_file_collects_module_name() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/basic/full_adder.sv"))
            .expect("parse full_adder");

        assert_eq!(source.modules.len(), 1);
        assert_eq!(source.modules[0].name, "full_adder");
        assert_eq!(source.modules[0].instantiations.len(), 3);
        assert_eq!(
            source.modules[0].instantiations[0].module_name,
            "half_adder"
        );
        assert_eq!(source.modules[0].instantiations[0].instance_name, "u_half1");
        assert!(
            source.modules[0].instantiations[0]
                .parameter_overrides
                .is_empty()
        );
    }

    #[test]
    fn parse_file_lowers_named_parameter_overrides() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/picorv32/picorv32.v"))
            .expect("parse picorv32");

        let module = source
            .modules
            .iter()
            .find(|module| module.name == "picorv32_wb")
            .expect("picorv32_wb module");
        let instance = module
            .instantiations
            .iter()
            .find(|instance| instance.instance_name == "picorv32_core")
            .expect("picorv32_core instance");

        assert!(
            module.unsupported.is_empty(),
            "unexpected unsupported entries: {:?}",
            module.unsupported
        );
        assert!(
            instance
                .parameter_overrides
                .iter()
                .any(|param| param.parameter_name == "ENABLE_COUNTERS")
        );
        assert!(
            instance
                .parameter_overrides
                .iter()
                .any(|param| param.parameter_name == "PROGADDR_RESET")
        );
    }

    #[test]
    fn parse_file_lowers_assignments_and_ports() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/basic/ternary_mux.sv"))
            .expect("parse ternary mux");

        let module = &source.modules[0];
        assert_eq!(module.ports.len(), 4);
        assert_eq!(module.continuous_assignments.len(), 1);
        assert!(module.unsupported.is_empty());
    }

    #[test]
    fn parse_str_lowers_modules_from_virtual_path() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/top.sv"),
                "module top(input logic a, output logic y); assign y = ~a; endmodule\n",
            )
            .expect("parse virtual source");

        assert_eq!(source.path, PathBuf::from("/virtual/design/top.sv"));
        assert_eq!(source.modules.len(), 1);
        let module = &source.modules[0];
        assert_eq!(module.name, "top");
        assert!(module.unsupported.is_empty());
        assert_eq!(module.ports.len(), 2);
        assert_eq!(module.continuous_assignments.len(), 1);
    }

    #[test]
    fn parse_file_lowers_always_comb_blocks() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/basic/mux_4to1_comb.sv"))
            .expect("parse mux_4to1_comb");

        let module = &source.modules[0];
        assert_eq!(module.proc_blocks.len(), 1);
        assert!(module.unsupported.is_empty());
    }

    #[test]
    fn parse_file_lowers_always_ff_blocks() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/basic/register_8bit.sv"))
            .expect("parse register_8bit");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.proc_blocks.len(), 1);
        assert_eq!(
            module.proc_blocks[0].kind,
            ProcBlockKind::AlwaysFf {
                clock: "clk".into(),
                async_reset: None,
            }
        );
        match &module.proc_blocks[0].body {
            Stmt::Block(statements) => match &statements[0] {
                Stmt::If { then_branch, .. } => match then_branch.as_ref() {
                    Stmt::Block(statements) => match &statements[0] {
                        Stmt::Assign { kind, .. } => {
                            assert_eq!(*kind, AssignmentKind::Nonblocking);
                        }
                        other => panic!("unexpected nested statement: {other:?}"),
                    },
                    other => panic!("unexpected then branch: {other:?}"),
                },
                other => panic!("unexpected first statement: {other:?}"),
            },
            other => panic!("unexpected always_ff body: {other:?}"),
        }
    }

    #[test]
    fn parse_file_lowers_memory_declaration_and_read() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/overture/overture_fetch.sv"))
            .expect("parse overture_fetch");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.memories.len(), 1);
        assert_eq!(module.memories[0].name, "rom");
        assert_eq!(module.memories[0].element_width(), 8);
        assert_eq!(module.memories[0].depth(), 256);
        match &module.continuous_assignments[0].expr {
            Expr::MemoryRead { memory, .. } => assert_eq!(memory, "rom"),
            other => panic!("unexpected memory read expression: {other:?}"),
        }
    }

    #[test]
    fn parse_file_lowers_always_ff_with_async_reset() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                "/virtual/top.sv",
                concat!(
                    "module top(input logic clk, input logic reset, output logic q);",
                    "always_ff @(posedge clk or posedge reset) begin ",
                    "if (reset) q <= 1'b0; else q <= ~q; ",
                    "end ",
                    "endmodule\n"
                ),
            )
            .expect("parse async reset top");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(
            module.proc_blocks[0].kind,
            ProcBlockKind::AlwaysFf {
                clock: "clk".into(),
                async_reset: Some("reset".into()),
            }
        );
    }

    #[test]
    fn parse_file_lowers_memory_element_write_in_always_ff() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/testing/memory_cpu_stub.sv"))
            .expect("parse memory_cpu_stub");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        match &module.proc_blocks[0].body {
            Stmt::Block(statements) => match &statements[0] {
                Stmt::If { else_branch, .. } => match else_branch.as_deref() {
                    Some(Stmt::If { then_branch, .. }) => match then_branch.as_ref() {
                        Stmt::Block(statements) => {
                            let case_stmt = statements
                                .iter()
                                .find_map(|statement| match statement {
                                    Stmt::Case { items, .. } => Some(items),
                                    _ => None,
                                })
                                .expect("run branch should contain a case statement");
                            match &case_stmt[2].body {
                                Stmt::Assign {
                                    kind,
                                    target: LValue::MemoryElement { memory, .. },
                                    ..
                                } => {
                                    assert_eq!(*kind, AssignmentKind::Nonblocking);
                                    assert_eq!(memory, "ram");
                                }
                                other => panic!("unexpected memory write statement: {other:?}"),
                            }
                        }
                        other => panic!("unexpected run branch body: {other:?}"),
                    },
                    other => panic!("unexpected else branch: {other:?}"),
                },
                other => panic!("unexpected first statement: {other:?}"),
            },
            other => panic!("unexpected always_ff body: {other:?}"),
        }
    }

    #[test]
    fn parse_file_lowers_concatenation_assignments_and_shared_ansi_ports() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/testing/016-Vector3.sv"))
            .expect("parse 016-Vector3");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.ports.len(), 10);
        match &module.continuous_assignments[0].target {
            LValue::Concat(items) => assert_eq!(items.len(), 4),
            other => panic!("unexpected concatenation target: {other:?}"),
        }
        match &module.continuous_assignments[0].expr {
            Expr::Concat(items) => assert_eq!(items.len(), 7),
            other => panic!("unexpected concatenation expression: {other:?}"),
        }
    }

    #[test]
    fn parse_file_lowers_replication_and_net_initializer() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/testing/019-Vector5.sv"))
            .expect("parse 019-Vector5");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.signals.len(), 2);
        assert_eq!(module.continuous_assignments.len(), 3);
        match &module.continuous_assignments[0].expr {
            Expr::Concat(items) => {
                assert_eq!(items.len(), 5);
                assert!(
                    items
                        .iter()
                        .all(|item| matches!(item, Expr::Repeat { count: 5, .. }))
                );
            }
            other => panic!("unexpected replicated concatenation: {other:?}"),
        }
        match &module.continuous_assignments[1].expr {
            Expr::Repeat { count, expr } => {
                assert_eq!(*count, 5);
                assert!(matches!(expr.as_ref(), Expr::Concat(items) if items.len() == 5));
            }
            other => panic!("unexpected multiple concatenation: {other:?}"),
        }
    }

    #[test]
    fn parse_str_preserves_storage_kinds_for_ports_signals_and_memories() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/storage_kinds.sv"),
                concat!(
                    "module top(input wire a, output logic y);\n",
                    "  wand pull_bus;\n",
                    "  logic state;\n",
                    "  logic [7:0] ram [0:3];\n",
                    "  assign pull_bus = a;\n",
                    "  assign y = pull_bus ^ state[0];\n",
                    "endmodule\n",
                ),
            )
            .expect("parse storage kind module");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(
            module.port("a").expect("input port").storage,
            StorageKind::Net(NetKind::Wire)
        );
        assert_eq!(
            module.port("y").expect("output port").storage,
            StorageKind::Variable
        );
        assert_eq!(
            module.signal_decl("pull_bus").expect("net decl").storage,
            StorageKind::Net(NetKind::Wand)
        );
        assert_eq!(
            module.signal_decl("state").expect("variable decl").storage,
            StorageKind::Variable
        );
        assert_eq!(
            module.memory_decl("ram").expect("memory decl").storage,
            StorageKind::Variable
        );
    }

    #[test]
    fn parse_str_prunes_constant_generate_else_if_chain() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/generate_top.sv"),
                r#"
module leaf_a(output logic y);
    assign y = 1'b1;
endmodule

module leaf_b(output logic y);
    assign y = 1'b0;
endmodule

module top #(parameter A = 0, parameter B = 1) (output logic y);
    generate if (A) begin : gen_a
        leaf_a u_leaf(.y(y));
    end else if (B) begin : gen_b
        leaf_b u_leaf(.y(y));
    end else begin : gen_c
        assign y = 1'b1;
    end endgenerate
endmodule
"#,
            )
            .expect("parse generated module");

        let module = source
            .modules
            .iter()
            .find(|module| module.name == "top")
            .expect("top module");
        assert!(module.unsupported.is_empty());
        assert_eq!(module.instantiations.len(), 1);
        assert_eq!(module.instantiations[0].module_name, "leaf_b");
        assert_eq!(module.instantiations[0].instance_name, "u_leaf");
        assert!(module.continuous_assignments.is_empty());
    }

    #[test]
    fn parse_str_prunes_generate_for_negated_localparam_condition() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/negated_generate.sv"),
                r#"
module top(output logic y);
    localparam NEG = -1;
    generate if (NEG) begin : gen_true
        assign y = 1'b1;
    end else begin : gen_false
        assign y = 1'b0;
    end endgenerate
endmodule
"#,
            )
            .expect("parse negated generate");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.continuous_assignments.len(), 1);
        match &module.continuous_assignments[0].expr {
            Expr::Literal(NumericLiteral { bits, .. }) => {
                assert_eq!(bits.to_u64_checked(), Some(1));
            }
            other => panic!("unexpected generated assignment: {other:?}"),
        }
    }

    #[test]
    fn parse_str_lowers_signedness_casts_in_constant_parameter_expressions() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/constant_signedness.sv"),
                r#"
module top(output logic y);
    localparam SIGNED_LT = $signed(8'hff) < $signed(1'b0);
    localparam UNSIGNED_EQ = $unsigned($signed(8'hff)) == 8'hff;
    generate if (SIGNED_LT && UNSIGNED_EQ) begin : gen_true
        assign y = 1'b1;
    end else begin : gen_false
        assign y = 1'b0;
    end endgenerate
endmodule
"#,
            )
            .expect("parse constant signedness generate");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.parameters.len(), 2);
        assert_eq!(module.continuous_assignments.len(), 1);
        match &module.continuous_assignments[0].expr {
            Expr::Literal(NumericLiteral { bits, .. }) => {
                assert_eq!(bits.to_u64_checked(), Some(1));
            }
            other => panic!("unexpected generated assignment: {other:?}"),
        }
    }

    #[test]
    fn parse_str_unrolls_procedural_for_loops_with_constant_indexed_part_selects() {
        fn collect_assignments<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Stmt>) {
            match stmt {
                Stmt::Assign { .. } => out.push(stmt),
                Stmt::Block(statements) => {
                    for statement in statements {
                        collect_assignments(statement, out);
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_assignments(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        collect_assignments(else_branch, out);
                    }
                }
                Stmt::Case { items, default, .. } => {
                    for item in items {
                        collect_assignments(&item.body, out);
                    }
                    if let Some(default) = default {
                        collect_assignments(default, out);
                    }
                }
                Stmt::Empty => {}
            }
        }

        fn expr_contains_ident(expr: &Expr, ident: &str) -> bool {
            match expr {
                Expr::Ident(name) => name == ident,
                Expr::Literal(_) => false,
                Expr::Concat(items) => items.iter().any(|item| expr_contains_ident(item, ident)),
                Expr::Repeat { expr, .. } => expr_contains_ident(expr, ident),
                Expr::MemoryRead { index, .. } => expr_contains_ident(index, ident),
                Expr::BitSelect { expr, .. } => expr_contains_ident(expr, ident),
                Expr::PartSelect { expr, .. } => expr_contains_ident(expr, ident),
                Expr::Unary { expr, .. } => expr_contains_ident(expr, ident),
                Expr::Binary { left, right, .. } => {
                    expr_contains_ident(left, ident) || expr_contains_ident(right, ident)
                }
                Expr::Ternary {
                    cond,
                    when_true,
                    when_false,
                } => {
                    expr_contains_ident(cond, ident)
                        || expr_contains_ident(when_true, ident)
                        || expr_contains_ident(when_false, ident)
                }
            }
        }

        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/procedural_for.sv"),
                r#"
module top #(parameter STRIDE = 2) (
    input logic [7:0] in,
    output logic [7:0] out
);
    integer i, j;

    always @* begin
        out = 8'h00;
        for (i = 0; i < 2; i = i + 1) begin
            for (j = 0; j < 4; j = j + STRIDE)
                out[j + i * 4 +: STRIDE] = in[j + i * 4 +: STRIDE] + i;
        end
    end
endmodule
"#,
            )
            .expect("parse procedural for module");

        let module = &source.modules[0];
        assert!(
            module.unsupported.is_empty(),
            "unexpected unsupported entries: {:?}",
            module.unsupported
        );
        assert_eq!(module.proc_blocks.len(), 1);

        let mut assignments = Vec::new();
        collect_assignments(&module.proc_blocks[0].body, &mut assignments);
        assert_eq!(assignments.len(), 5);

        let mut actual_ranges = Vec::new();
        let mut actual_increments = Vec::new();
        for assignment in &assignments[1..] {
            let Stmt::Assign { target, expr, .. } = assignment else {
                panic!("expected assignment");
            };
            let LValue::PartSelect { signal, msb, lsb } = target else {
                panic!("expected constant part-select target: {assignment:?}");
            };
            assert_eq!(signal, "out");
            actual_ranges.push((*msb, *lsb));
            assert!(
                !expr_contains_ident(expr, "i") && !expr_contains_ident(expr, "j"),
                "loop variables should be substituted away: {expr:?}"
            );
            let Expr::Binary { right, .. } = expr else {
                panic!("expected binary add expression: {expr:?}");
            };
            match right.as_ref() {
                Expr::Literal(NumericLiteral { bits, .. }) => {
                    actual_increments.push(bits.to_u64_checked().expect("literal increment"));
                }
                Expr::Unary { op, expr } if *op == crate::hir::UnaryOp::Signed => match expr
                    .as_ref()
                {
                    Expr::Literal(NumericLiteral { bits, .. }) => {
                        actual_increments.push(bits.to_u64_checked().expect("signed increment"));
                    }
                    other => panic!("unexpected signed increment expression: {other:?}"),
                },
                other => panic!("unexpected increment expression: {other:?}"),
            }
        }

        assert_eq!(actual_ranges, vec![(1, 0), (3, 2), (5, 4), (7, 6)]);
        assert_eq!(actual_increments, vec![0, 0, 1, 1]);
    }

    #[test]
    fn parse_str_preserves_comparison_operands_across_logical_and_rebalancing() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/precedence_if.sv"),
                r#"
module top(
    input logic [1:0] mem_wordsize,
    input logic [31:0] reg_op1,
    output logic trapit
);
    always_comb begin
        trapit = 1'b0;
        if (mem_wordsize == 0 && reg_op1[1:0] != 0)
            trapit = 1'b1;
    end
endmodule
"#,
            )
            .expect("parse precedence_if");

        let module = &source.modules[0];
        let Stmt::Block(statements) = &module.proc_blocks[0].body else {
            panic!("expected always_comb block");
        };
        let Stmt::If { cond, .. } = &statements[1] else {
            panic!("expected conditional statement");
        };
        match cond {
            Expr::Binary {
                left,
                op: BinaryOp::LogicalAnd,
                right,
            } => {
                assert!(matches!(
                    left.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::Eq,
                        ..
                    }
                ));
                assert!(matches!(
                    right.as_ref(),
                    Expr::Binary {
                        left: _,
                        op: BinaryOp::NotEq,
                        right: _
                    }
                ));
            }
            other => panic!("unexpected lowered condition: {other:?}"),
        }
    }

    #[test]
    fn parse_str_short_circuits_const_false_logical_and_during_pruning() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/short_circuit_prune.sv"),
                r#"
module top(
    input logic irq_pending,
    output logic seen
);
    localparam ENABLE_IRQ = 1'b0;
    logic [7:0] next_irq_pending;
    logic irq_active;
    integer irq_buserror;

    always_comb begin
        seen = 1'b0;
        if (ENABLE_IRQ && irq_pending && !irq_active) begin
            next_irq_pending[irq_buserror] = 1'b1;
            seen = 1'b1;
        end
    end
endmodule
"#,
            )
            .expect("parse short-circuit prune");

        let module = &source.modules[0];
        assert!(
            module.unsupported.is_empty(),
            "dead constant-false branch should be pruned before unsupported lowering: {:?}",
            module.unsupported
        );

        let Stmt::Block(statements) = &module.proc_blocks[0].body else {
            panic!("expected always_comb block");
        };
        assert_eq!(statements.len(), 2);
        assert!(matches!(statements[1], Stmt::Empty));
    }

    #[test]
    fn parse_str_treats_inert_debug_constructs_as_empty_statements() {
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_str(
                PathBuf::from("/virtual/design/inert_debug.sv"),
                r#"
module top(input logic a, output logic y);
    task empty_statement;
        begin end
    endtask

    always @* begin
        y = 1'b0;
        empty_statement;
        $display("debug");
        (* parallel_case *)
        case (1'b1)
            a: y = 1'b1;
            default: y = 1'b0;
        endcase
    end
endmodule
"#,
            )
            .expect("parse inert debug module");

        let module = &source.modules[0];
        assert!(module.unsupported.is_empty());
        assert_eq!(module.proc_blocks.len(), 1);
        match &module.proc_blocks[0].body {
            Stmt::Block(statements) => {
                assert!(matches!(statements[0], Stmt::Assign { .. }));
                assert!(matches!(statements[1], Stmt::Empty));
                assert!(matches!(statements[2], Stmt::Empty));
                assert!(matches!(statements[3], Stmt::Case { .. }));
            }
            other => panic!("unexpected always body: {other:?}"),
        }
    }
}
