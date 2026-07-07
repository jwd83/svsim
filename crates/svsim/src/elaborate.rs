use serde::Serialize;

use crate::diag::{Error, Result, SourceSpan};
use crate::expr_eval::{Value, eval_expr, resolve_parameter_defaults};
use crate::fast_hash::FxHashMap;
use crate::hir::{
    Expr, HirDesign, LValue, MemoryDecl, ModuleInstanceSummary, ModuleSummary, PackedRange,
    ParameterDecl, PortDecl, PortDirection, SignalDecl, StorageKind, expr_to_lvalue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RuntimeObjectShape {
    Bits { width: usize },
    Memory { element_width: usize, depth: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedDesign {
    pub top: ElaboratedInstance,
}

impl ElaboratedDesign {
    pub fn top_module(&self) -> &str {
        &self.top.module_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedInstance {
    pub module_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,
    pub path: Vec<String>,
    pub parameters: Vec<ElaboratedParameter>,
    /// Parameter values resolved against instance overrides, used to seed the
    /// simulation runtime. Internal: not part of the serialized design.
    #[serde(skip)]
    pub(crate) parameter_values: FxHashMap<String, Value>,
    pub ports: Vec<ElaboratedPort>,
    pub nets: Vec<ElaboratedNet>,
    pub variables: Vec<ElaboratedVariable>,
    pub memories: Vec<ElaboratedMemory>,
    pub bindings: Vec<ElaboratedPortBinding>,
    pub children: Vec<ElaboratedInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedParameter {
    pub name: String,
    pub range: Option<PackedRange>,
    pub width: usize,
    pub default_value: Expr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_expr: Option<Expr>,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedPort {
    pub name: String,
    pub direction: PortDirection,
    pub storage: StorageKind,
    pub range: Option<PackedRange>,
    pub shape: RuntimeObjectShape,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedNet {
    pub name: String,
    pub storage: StorageKind,
    pub range: Option<PackedRange>,
    pub shape: RuntimeObjectShape,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedVariable {
    pub name: String,
    pub storage: StorageKind,
    pub range: Option<PackedRange>,
    pub shape: RuntimeObjectShape,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedMemory {
    pub name: String,
    pub storage: StorageKind,
    pub element_range: Option<PackedRange>,
    pub index_range: PackedRange,
    pub shape: RuntimeObjectShape,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElaboratedPortBinding {
    pub port_name: String,
    pub direction: PortDirection,
    pub storage: StorageKind,
    pub shape: RuntimeObjectShape,
    pub expr: Expr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<LValue>,
    pub span: Option<SourceSpan>,
}

pub(crate) fn elaborate_design(hir: &HirDesign, top_module: &str) -> Result<ElaboratedDesign> {
    let mut stack = Vec::new();
    Ok(ElaboratedDesign {
        top: elaborate_instance(hir, top_module, None, Vec::new(), None, None, &mut stack)?,
    })
}

fn elaborate_instance(
    hir: &HirDesign,
    module_name: &str,
    instance_name: Option<String>,
    path: Vec<String>,
    instance_summary: Option<&ModuleInstanceSummary>,
    parent: Option<(&ModuleSummary, &FxHashMap<String, Value>)>,
    stack: &mut Vec<String>,
) -> Result<ElaboratedInstance> {
    if stack.iter().any(|name| name == module_name) {
        return Err(Error::Resolve(format!(
            "recursive instantiation detected at {} -> {}",
            stack.join(" -> "),
            module_name
        )));
    }

    let module = hir
        .module(module_name)
        .ok_or_else(|| Error::Resolve(format!("module '{}' was not compiled", module_name)))?;

    stack.push(module_name.to_owned());

    let parameter_values = elaborate_module_parameters(module, parent, instance_summary)?;

    let mut children = Vec::with_capacity(module.instantiations.len());
    for child in &module.instantiations {
        let mut child_path = path.clone();
        child_path.push(child.instance_name.clone());
        children.push(elaborate_instance(
            hir,
            &child.module_name,
            Some(child.instance_name.clone()),
            child_path,
            Some(child),
            Some((module, &parameter_values)),
            stack,
        )?);
    }

    stack.pop();

    Ok(ElaboratedInstance {
        module_name: module.name.clone(),
        instance_name,
        path,
        parameters: module
            .parameters
            .iter()
            .map(|parameter| elaborate_parameter(parameter, instance_summary))
            .collect(),
        parameter_values,
        ports: module.ports.iter().map(elaborate_port).collect(),
        nets: module
            .signals
            .iter()
            .filter(|signal| signal.is_net())
            .map(elaborate_net)
            .collect(),
        variables: module
            .signals
            .iter()
            .filter(|signal| signal.is_variable())
            .map(elaborate_variable)
            .collect(),
        memories: module.memories.iter().map(elaborate_memory).collect(),
        bindings: elaborate_bindings(module, instance_summary),
        children,
    })
}

fn elaborate_module_parameters(
    module: &ModuleSummary,
    parent: Option<(&ModuleSummary, &FxHashMap<String, Value>)>,
    instance: Option<&ModuleInstanceSummary>,
) -> Result<FxHashMap<String, Value>> {
    let empty_memories = FxHashMap::default();
    let mut values = FxHashMap::default();

    for param in &module.parameters {
        let value = if let Some(override_expr) = instance.and_then(|instance| {
            instance
                .parameter_overrides
                .iter()
                .find(|override_expr| override_expr.parameter_name == param.name)
        }) {
            let (parent_module, parent_values) = parent.ok_or_else(|| {
                Error::Resolve(format!(
                    "parameter override for '{}' on '{}' is missing parent module context",
                    param.name, module.name
                ))
            })?;
            eval_expr(
                &override_expr.expr,
                parent_module,
                parent_values,
                &empty_memories,
            )?
        } else {
            eval_expr(&param.default_value, module, &values, &empty_memories)?
        };
        values.insert(param.name.clone(), value.coerced_to(param.width()));
    }

    // Frozen parameters were baked into this module's HIR with their default
    // values at lowering time (see `ModuleSummary::frozen_parameters`). An
    // override that changes one — directly, or indirectly through a dependent
    // localparam — would silently disagree with the already-lowered design,
    // so reject it here. Overrides that resolve to the default value are fine.
    if instance.is_some_and(|instance| !instance.parameter_overrides.is_empty())
        && !module.frozen_parameters.is_empty()
    {
        let defaults = resolve_parameter_defaults(&module.parameters, module)?;
        for (name, frozen_construct) in &module.frozen_parameters {
            let (Some(actual), Some(default)) = (values.get(name), defaults.get(name)) else {
                continue;
            };
            if actual.logic() != default.logic() {
                let instance_name =
                    instance.map_or("<top>", |instance| instance.instance_name.as_str());
                return Err(Error::Resolve(format!(
                    "parameter '{name}' of module '{}' is frozen into {frozen_construct} at \
                     its lowering-time default; parameter overrides on instance \
                     '{instance_name}' would change its value",
                    module.name
                )));
            }
        }
    }

    Ok(values)
}

fn elaborate_parameter(
    parameter: &ParameterDecl,
    instance_summary: Option<&ModuleInstanceSummary>,
) -> ElaboratedParameter {
    let override_expr = instance_summary.and_then(|instance| {
        instance
            .parameter_overrides
            .iter()
            .find(|override_expr| override_expr.parameter_name == parameter.name)
            .map(|override_expr| override_expr.expr.clone())
    });

    ElaboratedParameter {
        name: parameter.name.clone(),
        range: parameter.range,
        width: parameter.width(),
        default_value: parameter.default_value.clone(),
        override_expr,
        span: parameter.span.clone(),
    }
}

fn elaborate_port(port: &PortDecl) -> ElaboratedPort {
    ElaboratedPort {
        name: port.name.clone(),
        direction: port.direction,
        storage: port.storage,
        range: port.range,
        shape: RuntimeObjectShape::Bits {
            width: port.width(),
        },
        span: port.span.clone(),
    }
}

fn elaborate_net(signal: &SignalDecl) -> ElaboratedNet {
    ElaboratedNet {
        name: signal.name.clone(),
        storage: signal.storage,
        range: signal.range,
        shape: RuntimeObjectShape::Bits {
            width: signal.width(),
        },
        span: signal.span.clone(),
    }
}

fn elaborate_variable(signal: &SignalDecl) -> ElaboratedVariable {
    ElaboratedVariable {
        name: signal.name.clone(),
        storage: signal.storage,
        range: signal.range,
        shape: RuntimeObjectShape::Bits {
            width: signal.width(),
        },
        span: signal.span.clone(),
    }
}

fn elaborate_memory(memory: &MemoryDecl) -> ElaboratedMemory {
    ElaboratedMemory {
        name: memory.name.clone(),
        storage: memory.storage,
        element_range: memory.element_range,
        index_range: memory.index_range,
        shape: RuntimeObjectShape::Memory {
            element_width: memory.element_width(),
            depth: memory.depth(),
        },
        span: memory.span.clone(),
    }
}

fn elaborate_bindings(
    module: &ModuleSummary,
    instance_summary: Option<&ModuleInstanceSummary>,
) -> Vec<ElaboratedPortBinding> {
    let Some(instance_summary) = instance_summary else {
        return Vec::new();
    };

    module
        .ports
        .iter()
        .filter_map(|port| {
            let connection = instance_summary
                .connections
                .iter()
                .find(|connection| connection.port_name == port.name)?;
            Some(ElaboratedPortBinding {
                port_name: port.name.clone(),
                direction: port.direction,
                storage: port.storage,
                shape: RuntimeObjectShape::Bits {
                    width: port.width(),
                },
                expr: connection.expr.clone(),
                target: expr_to_lvalue(&connection.expr),
                span: connection.span.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::Compiler;
    use crate::hir::{NetKind, StorageKind};

    use super::{RuntimeObjectShape, elaborate_design};

    #[test]
    fn elaboration_preserves_typed_runtime_objects_and_bindings() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/design/top.sv"),
                concat!(
                    "module child(input wire a, output logic y);\n",
                    "  wand shared;\n",
                    "  logic q;\n",
                    "  logic [7:0] ram [0:3];\n",
                    "  assign shared = a;\n",
                    "  assign y = shared;\n",
                    "endmodule\n",
                    "module top(input logic a, output logic y);\n",
                    "  wire link;\n",
                    "  child u_child(.a(a), .y(link));\n",
                    "  assign y = link;\n",
                    "endmodule\n",
                ),
            )
            .expect("compile elaboration fixture");

        let elaborated = elaborate_design(
            design.hir(),
            design.top_module().expect("compiled top module"),
        )
        .expect("elaborate design");

        assert_eq!(elaborated.top.module_name, "top");
        assert_eq!(elaborated.top.ports.len(), 2);
        assert!(elaborated.top.nets.iter().any(|net| net.name == "link"));
        assert_eq!(elaborated.top.children.len(), 1);

        let child = &elaborated.top.children[0];
        assert_eq!(child.instance_name.as_deref(), Some("u_child"));
        assert_eq!(child.path, vec!["u_child"]);
        assert_eq!(child.bindings.len(), 2);
        assert!(
            child
                .bindings
                .iter()
                .any(|binding| binding.port_name == "a" && binding.target.is_some())
        );
        assert!(
            child
                .bindings
                .iter()
                .any(|binding| binding.port_name == "y" && binding.target.is_some())
        );
        assert!(child.ports.iter().any(|port| port.name == "a"
            && port.storage == StorageKind::Net(NetKind::Wire)
            && port.shape == RuntimeObjectShape::Bits { width: 1 }));
        assert!(
            child
                .nets
                .iter()
                .any(|net| net.name == "shared" && net.storage == StorageKind::Net(NetKind::Wand))
        );
        assert!(child.variables.iter().any(|signal| signal.name == "q"));
        assert!(child.memories.iter().any(|memory| {
            memory.name == "ram"
                && memory.storage == StorageKind::Variable
                && memory.shape
                    == RuntimeObjectShape::Memory {
                        element_width: 8,
                        depth: 4,
                    }
        }));
    }
}
