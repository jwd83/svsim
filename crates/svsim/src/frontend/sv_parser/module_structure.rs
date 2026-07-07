//! Module-level lowering: source files, ANSI/non-ANSI module declarations,
//! parameter ports and localparams, generate regions, port declarations,
//! data/net declarations and dimensions, continuous assigns, and module
//! instantiations with parameter overrides.

use super::*;

#[derive(Debug, Default)]
pub(super) struct LoweredDeclarations {
    signals: Vec<SignalDecl>,
    memories: Vec<MemoryDecl>,
}

#[derive(Debug, Default)]
pub(super) struct LoweredNetDeclarations {
    signals: Vec<SignalDecl>,
    initializers: Vec<HirContinuousAssign>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AnsiPortContext {
    direction: HirPortDirection,
    storage: StorageKind,
    range: Option<PackedRange>,
}

pub(super) fn lower_source_file(syntax_tree: &SyntaxTree, path: &Path) -> Result<SourceFile> {
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

pub(super) fn lower_ansi_module(
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

pub(super) fn lower_nonansi_module(
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

pub(super) fn lower_non_port_module_item(
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

pub(super) fn lower_parameter_port_list(
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

pub(super) fn lower_parameter_port_declaration(
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

pub(super) fn lower_parameter_or_localparam_body(
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

pub(super) fn lower_parameter_or_localparam_declaration_into(
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
pub(super) enum ParameterOrLocal<'a> {
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

pub(super) fn lower_param_assignment(
    syntax_tree: &SyntaxTree,
    assignment: &sv_parser::ParamAssignment,
    range: Option<PackedRange>,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<ParameterDecl> {
    let (name, locate) = identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
        .ok_or_else(|| {
            unsupported(
                "failed to determine parameter name",
                span_of_node(path, assignment),
            )
        })?;

    if !assignment.nodes.1.is_empty() {
        return Err(unsupported(
            "parameter declarations with unpacked dimensions are not supported yet",
            span_of_node(path, assignment),
        ));
    }

    let (_, const_param_expr) = assignment.nodes.2.as_ref().ok_or_else(|| {
        unsupported(
            "parameter declarations without a default value are not supported yet",
            span_of_node(path, assignment),
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

pub(super) fn lower_module_or_generate_item(
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

pub(super) fn lower_generate_region(
    syntax_tree: &SyntaxTree,
    region: &sv_parser::GenerateRegion,
    path: &Path,
    module: &mut ModuleSummary,
) {
    for item in &region.nodes.1 {
        lower_generate_item(syntax_tree, item, path, module);
    }
}

pub(super) fn lower_generate_item(
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

pub(super) fn lower_generate_block(
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

pub(super) fn lower_conditional_generate_construct(
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

pub(super) fn lower_module_declaration_item(
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

pub(super) fn lower_ansi_port_declaration(
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
                        return Err(unsupported(
                            "interface ports are not supported yet",
                            span_of_node(path, &**decl),
                        ));
                    }
                }
            } else {
                inherited.ok_or_else(|| {
                    unsupported(
                        "ports must declare an explicit direction",
                        span_of_node(path, &**decl),
                    )
                })?
            };
            if !decl.nodes.2.is_empty() || decl.nodes.3.is_some() {
                return Err(unsupported(
                    "ANSI ports with unpacked dimensions or default values are not supported yet",
                    span_of_node(path, &**decl),
                ));
            }
            let (name, locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.1)).ok_or_else(
                    || {
                        unsupported(
                            "failed to determine ANSI port name",
                            span_of_node(path, &**decl),
                        )
                    },
                )?;
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
                inherited.ok_or_else(|| {
                    unsupported(
                        "ports must declare an explicit direction",
                        span_of_node(path, &**decl),
                    )
                })?
            };
            if !decl.nodes.2.is_empty() || decl.nodes.3.is_some() {
                return Err(unsupported(
                    "ANSI ports with unpacked dimensions or default values are not supported yet",
                    span_of_node(path, &**decl),
                ));
            }
            let (name, locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&decl.nodes.1)).ok_or_else(
                    || {
                        unsupported(
                            "failed to determine ANSI port name",
                            span_of_node(path, &**decl),
                        )
                    },
                )?;
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

pub(super) fn lower_port_direction(
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

pub(super) fn lower_net_port_range(
    syntax_tree: &SyntaxTree,
    port_type: &sv_parser::NetPortType,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    match port_type {
        sv_parser::NetPortType::DataType(data_type) => {
            lower_data_type_or_implicit_range(syntax_tree, &data_type.nodes.1, path, params)
        }
        _ => Err(unsupported(
            "unsupported net port type",
            span_of_node(path, port_type),
        )),
    }
}

pub(super) fn lower_net_port_storage_kind(
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

pub(super) fn lower_variable_port_range(
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

pub(super) fn lower_variable_port_storage_kind(
    port_type: &VariablePortType,
) -> LowerResult<StorageKind> {
    match &port_type.nodes.0 {
        sv_parser::VarDataType::DataType(_) | sv_parser::VarDataType::Var(_) => {
            Ok(StorageKind::Variable)
        }
    }
}

pub(super) fn lower_data_declaration(
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
                        span_of_node(path, &**decl),
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
                            unsupported(
                                "failed to determine variable declaration name",
                                span_of_node(path, &**decl),
                            )
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
        _ => Err(unsupported(
            "data declaration is not supported yet",
            span_of_node(path, decl),
        )),
    }
}

pub(super) fn lower_net_declaration(
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
                        span_of_node(path, &**decl),
                    ));
                }
                let (name, locate) =
                    identifier_name_from_node(syntax_tree, RefNode::from(&assignment.nodes.0))
                        .ok_or_else(|| {
                            unsupported(
                                "failed to determine net declaration name",
                                span_of_node(path, &**decl),
                            )
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
        _ => Err(unsupported(
            "net declaration is not supported yet",
            span_of_node(path, decl),
        )),
    }
}

pub(super) fn lower_data_type_or_implicit_range(
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

pub(super) fn lower_data_type_range(
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
            span_of_node(path, data_type),
        )),
    }
}

pub(super) fn lower_net_kind(net_type: &sv_parser::NetType) -> NetKind {
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

pub(super) fn lower_implicit_data_type_range(
    syntax_tree: &SyntaxTree,
    data_type: &ImplicitDataType,
    path: &Path,
    params: &[ParameterDecl],
) -> LowerResult<Option<PackedRange>> {
    lower_packed_dimensions(syntax_tree, &data_type.nodes.1, path, params)
}

pub(super) fn lower_unpacked_dimensions(
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
        [UnpackedDimension::Expression(dimension)] => Err(unsupported(
            "unsized unpacked dimensions are not supported yet",
            span_of_node(path, &**dimension),
        )),
        _ => Err(unsupported(
            "multiple unpacked dimensions are not supported yet",
            unpacked_dimensions
                .first()
                .and_then(|dimension| span_of_node(path, dimension)),
        )),
    }
}

pub(super) fn lower_variable_dimensions(
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
            dimensions
                .first()
                .and_then(|dimension| span_of_node(path, dimension)),
        )),
        _ => Err(unsupported(
            "multiple unpacked dimensions are not supported yet",
            dimensions
                .first()
                .and_then(|dimension| span_of_node(path, dimension)),
        )),
    }
}

pub(super) fn lower_packed_dimensions(
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
            packed_dimensions
                .first()
                .and_then(|dimension| span_of_node(path, dimension)),
        )),
    }
}

pub(super) fn lower_continuous_assign(
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

pub(super) fn lower_module_instantiation(
    syntax_tree: &SyntaxTree,
    instantiation: &ModuleInstantiation,
    module: &ModuleSummary,
    path: &Path,
) -> LowerResult<Vec<ModuleInstanceSummary>> {
    let (module_name, _) =
        identifier_name_from_node(syntax_tree, RefNode::from(&instantiation.nodes.0)).ok_or_else(
            || {
                unsupported(
                    "failed to determine instantiated module name",
                    span_of_node(path, instantiation),
                )
            },
        )?;
    let mut parameter_overrides = Vec::new();
    if let Some(parameter_value_assignment) = &instantiation.nodes.1 {
        let Some(assignments) = parameter_value_assignment.nodes.1.nodes.1.as_ref() else {
            return Err(unsupported(
                "empty parameter override lists are not supported yet",
                span_of_node(path, instantiation),
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
                    .ok_or_else(|| {
                        unsupported(
                            "failed to determine parameter name",
                            span_of_node(path, instantiation),
                        )
                    })?;
            let expr = assignment
                .nodes
                .2
                .nodes
                .1
                .as_ref()
                .ok_or_else(|| {
                    unsupported(
                        "named parameter overrides must provide an expression",
                        span_of_node(path, instantiation),
                    )
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
                .ok_or_else(|| {
                    unsupported(
                        "failed to determine instance name",
                        span_of_node(path, instantiation),
                    )
                })?;
        let Some(port_connections) = instance.nodes.1.nodes.1.as_ref() else {
            return Err(unsupported(
                "module instantiations must use explicit named port connections",
                span_of_node(path, instantiation),
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
                    span_of_node(path, instantiation),
                ));
            };
            let (port_name, port_locate) =
                identifier_name_from_node(syntax_tree, RefNode::from(&connection.nodes.2))
                    .ok_or_else(|| {
                        unsupported(
                            "failed to determine connected port name",
                            span_of_node(path, instantiation),
                        )
                    })?;
            let expr = connection
                .nodes
                .3
                .as_ref()
                .and_then(|paren| paren.nodes.1.as_ref())
                .ok_or_else(|| {
                    unsupported(
                        "named port connections must provide an expression",
                        span_of_node(path, instantiation),
                    )
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

pub(super) fn lower_param_expression(
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
            span_of_node(path, expr),
        )),
    }
}

pub(super) fn lower_mintypmax_expression_to_expr(
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
