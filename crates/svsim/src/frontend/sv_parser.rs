use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sv_parser::{
    AnsiPortDeclaration, BinaryOperator, CondPredicate, ConstantExpression,
    ConstantPartSelectRange, ConstantRange, ConstantSelect, ContinuousAssign, DataDeclaration,
    DataType, DataTypeOrImplicit, Define, Defines, Expression, HierarchicalIdentifier,
    ImplicitDataType, ListOfPortConnections, Locate, ModuleDeclarationAnsi,
    ModuleDeclarationNonansi, ModuleInstantiation, ModuleOrGenerateItem,
    ModuleOrGenerateItemDeclaration, NamedPortConnection, NetDeclaration, NetLvalue,
    NonPortModuleItem, PackageOrGenerateItemDeclaration, PartSelectRange, PortDirection, Primary,
    PsOrHierarchicalNetIdentifier, RefNode, Select, SyntaxTree, UnaryOperator, VariableAssignment,
    VariableLvalue, VariablePortType, parse_sv, unwrap_node,
};

use crate::diag::{Diagnostic, Error, Result, SourceSpan};
use crate::hir::{
    BinaryOp, ContinuousAssign as HirContinuousAssign, Expr, LValue, ModuleDeclStyle,
    ModuleInstanceSummary, ModuleSummary, NamedPortConnection as HirNamedPortConnection,
    NumericLiteral, PackedRange, PortDecl, PortDirection as HirPortDirection, SignalDecl,
    SourceFile, UnaryOp,
};

type LowerResult<T> = std::result::Result<T, Diagnostic>;

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
        let (syntax_tree, _) = parse_sv(path, &defines, &self.include_paths, false, false)
            .map_err(|error| {
                Error::Parse(format!("failed to parse {}: {error}", path.display()))
            })?;

        let mut modules = Vec::new();
        for node in &syntax_tree {
            match node {
                RefNode::ModuleDeclarationAnsi(decl) => {
                    modules.push(lower_ansi_module(&syntax_tree, decl, path)?);
                }
                RefNode::ModuleDeclarationNonansi(decl) => {
                    modules.push(lower_nonansi_module(&syntax_tree, decl, path)?);
                }
                _ => {}
            }
        }

        Ok(SourceFile {
            path: path.to_path_buf(),
            modules,
        })
    }
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
        signals: Vec::new(),
        continuous_assignments: Vec::new(),
        instantiations: Vec::new(),
        unsupported: Vec::new(),
    };

    if let Some(port_decls) = decl.nodes.0.nodes.6.as_ref() {
        if let Some(list) = port_decls.nodes.0.nodes.1.as_ref() {
            for port_decl in list.contents() {
                match lower_ansi_port_declaration(syntax_tree, &port_decl.1, path) {
                    Ok(port) => module.ports.push(port),
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
        signals: Vec::new(),
        continuous_assignments: Vec::new(),
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

fn lower_module_or_generate_item(
    syntax_tree: &SyntaxTree,
    item: &ModuleOrGenerateItem,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match item {
        ModuleOrGenerateItem::Module(item) => {
            match lower_module_instantiation(syntax_tree, &item.nodes.1, path) {
                Ok(instantiations) => module.instantiations.extend(instantiations),
                Err(diag) => module.unsupported.push(diag),
            }
        }
        ModuleOrGenerateItem::ModuleItem(item) => match &item.nodes.1 {
            sv_parser::ModuleCommonItem::ModuleOrGenerateItemDeclaration(decl) => {
                lower_module_declaration_item(syntax_tree, decl, path, module);
            }
            sv_parser::ModuleCommonItem::ContinuousAssign(assign) => {
                match lower_continuous_assign(syntax_tree, assign, path) {
                    Ok(assignments) => module.continuous_assignments.extend(assignments),
                    Err(diag) => module.unsupported.push(diag),
                }
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

fn lower_module_declaration_item(
    syntax_tree: &SyntaxTree,
    decl: &ModuleOrGenerateItemDeclaration,
    path: &Path,
    module: &mut ModuleSummary,
) {
    match decl {
        ModuleOrGenerateItemDeclaration::PackageOrGenerateItemDeclaration(decl) => match &**decl {
            PackageOrGenerateItemDeclaration::DataDeclaration(decl) => {
                match lower_data_declaration(syntax_tree, decl, path) {
                    Ok(signals) => module.signals.extend(signals),
                    Err(diag) => module.unsupported.push(diag),
                }
            }
            PackageOrGenerateItemDeclaration::NetDeclaration(decl) => {
                match lower_net_declaration(syntax_tree, decl, path) {
                    Ok(signals) => module.signals.extend(signals),
                    Err(diag) => module.unsupported.push(diag),
                }
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
) -> LowerResult<PortDecl> {
    match decl {
        AnsiPortDeclaration::Net(decl) => {
            let header = decl
                .nodes
                .0
                .as_ref()
                .ok_or_else(|| unsupported("ports must declare an explicit direction", None))?;
            let (direction, range) = match header {
                sv_parser::NetPortHeaderOrInterfacePortHeader::NetPortHeader(header) => (
                    lower_port_direction(header.nodes.0.as_ref(), path)?,
                    lower_net_port_range(syntax_tree, &header.nodes.1, path)?,
                ),
                sv_parser::NetPortHeaderOrInterfacePortHeader::InterfacePortHeader(_) => {
                    return Err(unsupported("interface ports are not supported yet", None));
                }
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
            Ok(PortDecl {
                name,
                direction,
                range,
                span: Some(span_from_locate(path, locate)),
            })
        }
        AnsiPortDeclaration::Variable(decl) => {
            let header = decl
                .nodes
                .0
                .as_ref()
                .ok_or_else(|| unsupported("ports must declare an explicit direction", None))?;
            let direction = lower_port_direction(header.nodes.0.as_ref(), path)?;
            let range = lower_variable_port_range(syntax_tree, &header.nodes.1, path)?;
            if !decl.nodes.2.is_empty() || decl.nodes.3.is_some() {
                return Err(unsupported(
                    "ANSI ports with unpacked dimensions or default values are not supported yet",
                    None,
                ));
            }
            let (name, locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.1))
                    .ok_or_else(|| unsupported("failed to determine ANSI port name", None))?;
            Ok(PortDecl {
                name,
                direction,
                range,
                span: Some(span_from_locate(path, locate)),
            })
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
) -> LowerResult<Option<PackedRange>> {
    match port_type {
        sv_parser::NetPortType::DataType(data_type) => {
            lower_data_type_or_implicit_range(syntax_tree, &data_type.nodes.1, path)
        }
        _ => Err(unsupported("unsupported net port type", None)),
    }
}

fn lower_variable_port_range(
    syntax_tree: &SyntaxTree,
    port_type: &VariablePortType,
    path: &Path,
) -> LowerResult<Option<PackedRange>> {
    match &port_type.nodes.0 {
        sv_parser::VarDataType::DataType(data_type) => {
            lower_data_type_range(syntax_tree, data_type, path)
        }
        sv_parser::VarDataType::Var(var_type) => {
            lower_data_type_or_implicit_range(syntax_tree, &var_type.nodes.1, path)
        }
    }
}

fn lower_data_declaration(
    syntax_tree: &SyntaxTree,
    decl: &DataDeclaration,
    path: &Path,
) -> LowerResult<Vec<SignalDecl>> {
    match decl {
        DataDeclaration::Variable(decl) => {
            let range = lower_data_type_or_implicit_range(syntax_tree, &decl.nodes.3, path)?;
            let mut signals = Vec::new();
            for assignment in decl.nodes.4.nodes.0.contents() {
                let sv_parser::VariableDeclAssignment::Variable(assignment) = assignment else {
                    return Err(unsupported(
                        "complex variable declarations are not supported yet",
                        None,
                    ));
                };
                if !assignment.nodes.1.is_empty() || assignment.nodes.2.is_some() {
                    return Err(unsupported(
                        "variable declarations with unpacked dimensions or initializers are not supported yet",
                        None,
                    ));
                }
                let (name, locate) =
                    identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
                        .ok_or_else(|| {
                            unsupported("failed to determine variable declaration name", None)
                        })?;
                signals.push(SignalDecl {
                    name,
                    range,
                    span: Some(span_from_locate(path, locate)),
                });
            }
            Ok(signals)
        }
        _ => Err(unsupported("data declaration is not supported yet", None)),
    }
}

fn lower_net_declaration(
    syntax_tree: &SyntaxTree,
    decl: &NetDeclaration,
    path: &Path,
) -> LowerResult<Vec<SignalDecl>> {
    match decl {
        NetDeclaration::NetType(decl) => {
            let range = lower_data_type_or_implicit_range(syntax_tree, &decl.nodes.3, path)?;
            let mut signals = Vec::new();
            for assignment in decl.nodes.5.nodes.0.contents() {
                if !assignment.nodes.1.is_empty() || assignment.nodes.2.is_some() {
                    return Err(unsupported(
                        "net declarations with unpacked dimensions or initializers are not supported yet",
                        None,
                    ));
                }
                let (name, locate) =
                    identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
                        .ok_or_else(|| {
                            unsupported("failed to determine net declaration name", None)
                        })?;
                signals.push(SignalDecl {
                    name,
                    range,
                    span: Some(span_from_locate(path, locate)),
                });
            }
            Ok(signals)
        }
        _ => Err(unsupported("net declaration is not supported yet", None)),
    }
}

fn lower_data_type_or_implicit_range(
    syntax_tree: &SyntaxTree,
    data_type: &DataTypeOrImplicit,
    path: &Path,
) -> LowerResult<Option<PackedRange>> {
    match data_type {
        DataTypeOrImplicit::DataType(data_type) => {
            lower_data_type_range(syntax_tree, data_type, path)
        }
        DataTypeOrImplicit::ImplicitDataType(data_type) => {
            lower_implicit_data_type_range(syntax_tree, data_type, path)
        }
    }
}

fn lower_data_type_range(
    syntax_tree: &SyntaxTree,
    data_type: &DataType,
    path: &Path,
) -> LowerResult<Option<PackedRange>> {
    match data_type {
        DataType::Vector(data_type) => {
            lower_packed_dimensions(syntax_tree, &data_type.nodes.2, path)
        }
        DataType::Atom(_) => Ok(None),
        DataType::Type(data_type) => lower_packed_dimensions(syntax_tree, &data_type.nodes.2, path),
        _ => Err(unsupported(
            "data type is outside the current executable subset",
            None,
        )),
    }
}

fn lower_implicit_data_type_range(
    syntax_tree: &SyntaxTree,
    data_type: &ImplicitDataType,
    path: &Path,
) -> LowerResult<Option<PackedRange>> {
    lower_packed_dimensions(syntax_tree, &data_type.nodes.1, path)
}

fn lower_packed_dimensions(
    syntax_tree: &SyntaxTree,
    packed_dimensions: &[sv_parser::PackedDimension],
    path: &Path,
) -> LowerResult<Option<PackedRange>> {
    match packed_dimensions {
        [] => Ok(None),
        [sv_parser::PackedDimension::Range(range)] => {
            lower_constant_range(syntax_tree, &range.nodes.0.nodes.1, path).map(Some)
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
    path: &Path,
) -> LowerResult<Vec<HirContinuousAssign>> {
    match assign {
        ContinuousAssign::Net(assign) => {
            let mut lowered = Vec::new();
            for assignment in assign.nodes.3.nodes.0.contents() {
                lowered.push(HirContinuousAssign {
                    target: lower_net_lvalue(syntax_tree, &assignment.nodes.0, path)?,
                    expr: lower_expression(syntax_tree, &assignment.nodes.2, path)?,
                    span: None,
                });
            }
            Ok(lowered)
        }
        ContinuousAssign::Variable(assign) => {
            let mut lowered = Vec::new();
            for assignment in assign.nodes.2.nodes.0.contents() {
                lowered.push(HirContinuousAssign {
                    target: lower_variable_assignment_lvalue(syntax_tree, assignment, path)?,
                    expr: lower_expression(syntax_tree, &assignment.nodes.2, path)?,
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
    path: &Path,
) -> LowerResult<Vec<ModuleInstanceSummary>> {
    if instantiation.nodes.1.is_some() {
        return Err(unsupported(
            "parameterized module instantiations are not supported yet",
            None,
        ));
    }

    let (module_name, _) =
        identifier_name_from_node(syntax_tree, RefNode::from(&instantiation.nodes.0))
            .ok_or_else(|| unsupported("failed to determine instantiated module name", None))?;
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
                .and_then(|expr| lower_expression(syntax_tree, expr, path))?;
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
            connections: lowered_connections,
        });
    }

    Ok(instances)
}

fn lower_expression(syntax_tree: &SyntaxTree, expr: &Expression, path: &Path) -> LowerResult<Expr> {
    match expr {
        Expression::Primary(primary) => lower_primary(syntax_tree, primary, path),
        Expression::Unary(expr) => {
            let op = lower_unary_operator(syntax_tree, &expr.nodes.0)?;
            Ok(Expr::Unary {
                op,
                expr: Box::new(lower_primary(syntax_tree, &expr.nodes.2, path)?),
            })
        }
        Expression::Binary(expr) => {
            let op = lower_binary_operator(syntax_tree, &expr.nodes.1)?;
            Ok(Expr::Binary {
                left: Box::new(lower_expression(syntax_tree, &expr.nodes.0, path)?),
                op,
                right: Box::new(lower_expression(syntax_tree, &expr.nodes.3, path)?),
            })
        }
        Expression::ConditionalExpression(expr) => Ok(Expr::Ternary {
            cond: Box::new(lower_cond_predicate(syntax_tree, &expr.nodes.0, path)?),
            when_true: Box::new(lower_expression(syntax_tree, &expr.nodes.3, path)?),
            when_false: Box::new(lower_expression(syntax_tree, &expr.nodes.5, path)?),
        }),
        _ => Err(unsupported(
            "expression is outside the current executable subset",
            None,
        )),
    }
}

fn lower_primary(syntax_tree: &SyntaxTree, primary: &Primary, path: &Path) -> LowerResult<Expr> {
    match primary {
        Primary::PrimaryLiteral(literal) => lower_literal(syntax_tree, literal),
        Primary::Hierarchical(primary) => {
            let (name, _) = lower_hierarchical_identifier(
                syntax_tree,
                &primary.nodes.1,
                "hierarchical expressions",
            )?;
            lower_expr_select(syntax_tree, Expr::Ident(name), &primary.nodes.2, path)
        }
        Primary::MintypmaxExpression(expr) => {
            lower_mintypmax_expression(syntax_tree, &expr.nodes.0.nodes.1, path)
        }
        _ => Err(unsupported("primary expression is not supported yet", None)),
    }
}

fn lower_mintypmax_expression(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::MintypmaxExpression,
    path: &Path,
) -> LowerResult<Expr> {
    match expr {
        sv_parser::MintypmaxExpression::Expression(expr) => {
            lower_expression(syntax_tree, expr, path)
        }
        sv_parser::MintypmaxExpression::Ternary(expr) => Ok(Expr::Ternary {
            cond: Box::new(lower_expression(syntax_tree, &expr.nodes.0, path)?),
            when_true: Box::new(lower_expression(syntax_tree, &expr.nodes.2, path)?),
            when_false: Box::new(lower_expression(syntax_tree, &expr.nodes.4, path)?),
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
                "'0" => 0,
                "'1" => 1,
                _ => return Err(unsupported("unsupported unbased unsized literal", None)),
            };
            Ok(Expr::Literal(NumericLiteral { bits, width: None }))
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
                let bits = locate_usize(syntax_tree, &number.nodes.0)? as u64;
                Ok(NumericLiteral { bits, width: None })
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
    path: &Path,
) -> LowerResult<Expr> {
    if select.nodes.0.is_some() {
        return Err(unsupported("member selections are not supported yet", None));
    }

    let mut expr = base;
    let bit_selects = &select.nodes.1.nodes.0;
    match bit_selects.as_slice() {
        [] => {}
        [index] => {
            expr = Expr::BitSelect {
                expr: Box::new(expr),
                index: lower_usize_expression(syntax_tree, &index.nodes.1, path)?,
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
        let (msb, lsb) = lower_part_select_range(syntax_tree, &range.nodes.1, path)?;
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
        _ => Err(unsupported(
            "complex net lvalues are not supported yet",
            None,
        )),
    }
}

fn lower_variable_assignment_lvalue(
    syntax_tree: &SyntaxTree,
    assignment: &VariableAssignment,
    path: &Path,
) -> LowerResult<LValue> {
    lower_variable_lvalue(syntax_tree, &assignment.nodes.0, path)
}

fn lower_variable_lvalue(
    syntax_tree: &SyntaxTree,
    lvalue: &VariableLvalue,
    path: &Path,
) -> LowerResult<LValue> {
    match lvalue {
        VariableLvalue::Identifier(lvalue) => {
            let (name, _) = lower_hierarchical_identifier(
                syntax_tree,
                &lvalue.nodes.1.nodes.0,
                "variable lvalues",
            )?;
            lower_select_lvalue(syntax_tree, name, &lvalue.nodes.2, path)
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
                index: lower_usize_expression(syntax_tree, &index.nodes.1, path)?,
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
        let (msb, lsb) = lower_part_select_range(syntax_tree, &range.nodes.1, path)?;
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
            lower_expression(syntax_tree, expr, path)
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
        _ => Err(unsupported("binary operator is not supported yet", None)),
    }
}

fn lower_constant_range(
    syntax_tree: &SyntaxTree,
    range: &ConstantRange,
    path: &Path,
) -> LowerResult<PackedRange> {
    Ok(PackedRange {
        msb: lower_usize_constant_expression(syntax_tree, &range.nodes.0, path)?,
        lsb: lower_usize_constant_expression(syntax_tree, &range.nodes.2, path)?,
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
    path: &Path,
) -> LowerResult<(usize, usize)> {
    match range {
        PartSelectRange::ConstantRange(range) => Ok((
            lower_usize_constant_expression(syntax_tree, &range.nodes.0, path)?,
            lower_usize_constant_expression(syntax_tree, &range.nodes.2, path)?,
        )),
        PartSelectRange::IndexedRange(_) => Err(unsupported(
            "indexed part selects are not supported yet",
            None,
        )),
    }
}

fn lower_usize_expression(
    syntax_tree: &SyntaxTree,
    expr: &Expression,
    path: &Path,
) -> LowerResult<usize> {
    let Expr::Literal(literal) = lower_expression(syntax_tree, expr, path)? else {
        return Err(unsupported(
            "only constant bit and part select indices are supported",
            None,
        ));
    };
    Ok(literal.bits as usize)
}

fn lower_usize_constant_expression(
    syntax_tree: &SyntaxTree,
    expr: &ConstantExpression,
    path: &Path,
) -> LowerResult<usize> {
    match expr {
        ConstantExpression::ConstantPrimary(primary) => match &**primary {
            sv_parser::ConstantPrimary::PrimaryLiteral(literal) => {
                let expr = lower_literal(syntax_tree, literal)?;
                let Expr::Literal(literal) = expr else {
                    unreachable!();
                };
                Ok(literal.bits as usize)
            }
            sv_parser::ConstantPrimary::MintypmaxExpression(expr) => {
                lower_usize_constant_mintypmax_expression(syntax_tree, &expr.nodes.0.nodes.1, path)
            }
            _ => Err(unsupported(
                "constant expression is outside the supported subset",
                None,
            )),
        },
        ConstantExpression::Ternary(_)
        | ConstantExpression::Binary(_)
        | ConstantExpression::Unary(_) => Err(unsupported(
            "only literal constant expressions are supported in ranges",
            None,
        )),
    }
}

fn lower_usize_constant_mintypmax_expression(
    syntax_tree: &SyntaxTree,
    expr: &sv_parser::ConstantMintypmaxExpression,
    path: &Path,
) -> LowerResult<usize> {
    match expr {
        sv_parser::ConstantMintypmaxExpression::Unary(expr) => {
            lower_usize_constant_expression(syntax_tree, expr, path)
        }
        sv_parser::ConstantMintypmaxExpression::Ternary(_) => Err(unsupported(
            "ternary constant ranges are not supported yet",
            None,
        )),
    }
}

fn parse_based_value(syntax_tree: &SyntaxTree, locate: &Locate, radix: u32) -> LowerResult<u64> {
    let text = syntax_tree
        .get_str(locate)
        .ok_or_else(|| unsupported("failed to read numeric literal text", None))?;
    let cleaned = text.replace('_', "");
    if cleaned.contains(['x', 'X', 'z', 'Z', '?']) {
        return Err(unsupported(
            "x/z numeric literal digits are not supported yet",
            None,
        ));
    }
    u64::from_str_radix(&cleaned, radix)
        .map_err(|_| unsupported("failed to parse numeric literal", None))
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
}
