//! Hierarchical module state: instantiation, parameter elaboration, runtime
//! object layout, signal bindings, and frame synchronization.

use super::*;

#[derive(Debug, Clone)]
pub(super) struct ModuleState {
    pub(super) module_name: String,
    pub(super) parameter_values: FxHashMap<String, Value>,
    pub(super) signals: FxHashMap<String, SignalBinding>,
    pub(super) memories: FxHashMap<String, MemoryState>,
    pub(super) previous_clocks: FxHashMap<String, bool>,
    pub(super) legacy_rom: Option<LegacyRomState>,
    pub(super) children: Vec<ChildState>,
    /// Signals on net-storage runtime objects that need their driver
    /// refreshed every settle pass (see `NetSpecialAction`); precomputed so
    /// the per-pass commit only walks dirty names plus this set.
    pub(super) net_specials: FxHashMap<String, (SignalBinding, NetSpecialAction)>,
    /// Signals whose whole value is driven by a child instance's output
    /// sink; the module's own commit skips them (the child drive wins).
    pub(super) child_output_driven: FxHashSet<String>,
}

/// Per-pass driver refresh policy for a signal on a net-storage object.
/// Nets resolve from the drivers staged in the current pass only, so these
/// must be re-staged every pass or an untouched net would float to Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NetSpecialAction {
    /// Variable-storage signal backing a net object: its value replaces the
    /// object's driver list outright (and is written to the frame).
    ReplaceDriver,
    /// Net signal with a procedural driver: its current value is staged as a
    /// driver unless something else already drove the object this pass.
    StageIfUndriven,
}

#[derive(Debug, Clone)]
pub(super) struct ChildState {
    pub(super) instance_name: String,
    pub(super) input_drivers: Vec<PortExprDriver>,
    pub(super) output_sinks: Vec<PortSink>,
    pub(super) state: Box<ModuleState>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RuntimeObjectLayout {
    pub(super) width: usize,
    pub(super) storage: StorageKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SignalBinding {
    pub(super) object_id: usize,
    pub(super) view_width: usize,
}

impl SignalBinding {
    pub(super) fn with_view_width(self, view_width: usize) -> Self {
        Self {
            object_id: self.object_id,
            view_width: view_width.max(1),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PortExprDriver {
    pub(super) port_name: String,
    pub(super) expr: Expr,
}

#[derive(Debug, Clone)]
pub(super) struct PortSink {
    pub(super) port_name: String,
    pub(super) target: LValue,
}

pub(super) type NetDriverTable = FxHashMap<usize, Vec<NetDriver>>;

pub(super) fn top_module<'a>(hir: &'a HirDesign, module_name: &str) -> Result<&'a ModuleSummary> {
    resolve_supported_module(hir, module_name)
}

pub(super) fn instantiate_module_state(
    hir: &HirDesign,
    elaborated: &ElaboratedInstance,
    provided_ports: FxHashMap<String, SignalBinding>,
    objects: &mut Vec<RuntimeObjectLayout>,
    stack: &mut Vec<String>,
) -> Result<ModuleState> {
    if stack.iter().any(|name| name == &elaborated.module_name) {
        return Err(Error::Unsupported(format!(
            "recursive instantiation detected at {} -> {}",
            stack.join(" -> "),
            elaborated.module_name
        )));
    }

    let module = resolve_supported_module(hir, &elaborated.module_name)?;
    let parameter_values = elaborated.parameter_values.clone();

    stack.push(elaborated.module_name.clone());

    let mut signals = FxHashMap::default();
    for port in &elaborated.ports {
        let width = runtime_bits_width(port.shape)?;
        let binding = provided_ports
            .get(&port.name)
            .copied()
            .unwrap_or_else(|| allocate_runtime_object(objects, width, port.storage));
        signals.insert(port.name.clone(), binding);
    }
    for net in &elaborated.nets {
        signals.insert(
            net.name.clone(),
            allocate_runtime_object(objects, runtime_bits_width(net.shape)?, net.storage),
        );
    }
    for variable in &elaborated.variables {
        signals.insert(
            variable.name.clone(),
            allocate_runtime_object(
                objects,
                runtime_bits_width(variable.shape)?,
                variable.storage,
            ),
        );
    }

    let mut children = Vec::with_capacity(elaborated.children.len());
    for child in &elaborated.children {
        let mut child_ports = FxHashMap::default();
        let mut input_drivers = Vec::new();
        let mut output_sinks = Vec::new();

        for port in &child.ports {
            let port_width = runtime_bits_width(port.shape)?;
            let binding = if let Some(binding) = child
                .bindings
                .iter()
                .find(|binding| binding.port_name == port.name)
            {
                if let Some(parent_name) =
                    aliasable_parent_signal_name(module, port.direction, binding.target.as_ref())
                {
                    let parent_binding = signals.get(parent_name).copied().ok_or_else(|| {
                        Error::Resolve(format!(
                            "signal '{}' is not declared in '{}'",
                            parent_name, module.name
                        ))
                    })?;
                    grow_runtime_object(objects, parent_binding.object_id, port_width);
                    parent_binding.with_view_width(port_width)
                } else {
                    let binding_slot = allocate_runtime_object(objects, port_width, port.storage);
                    match port.direction {
                        PortDirection::Input => input_drivers.push(PortExprDriver {
                            port_name: port.name.clone(),
                            expr: binding.expr.clone(),
                        }),
                        PortDirection::Output => output_sinks.push(PortSink {
                            port_name: port.name.clone(),
                            target: binding.target.clone().ok_or_else(|| {
                                Error::Resolve(format!(
                                    "instance '{}' output port '{}' is missing a target",
                                    child.instance_name.as_deref().unwrap_or("<top>"),
                                    port.name
                                ))
                            })?,
                        }),
                        PortDirection::Inout | PortDirection::Ref => {
                            return Err(Error::Unsupported(format!(
                                "module '{}' uses unsupported port direction on '{}'",
                                child.module_name, port.name
                            )));
                        }
                    }
                    binding_slot
                }
            } else {
                if matches!(port.direction, PortDirection::Input) {
                    return Err(Error::Resolve(format!(
                        "instance '{}' is missing a connection for input port '{}' on module '{}'",
                        child.instance_name.as_deref().unwrap_or("<top>"),
                        port.name,
                        child.module_name
                    )));
                }
                allocate_runtime_object(objects, port_width, port.storage)
            };
            child_ports.insert(port.name.clone(), binding);
        }

        children.push(ChildState {
            instance_name: child
                .instance_name
                .clone()
                .expect("child elaborated instances always carry a name"),
            input_drivers,
            output_sinks,
            state: Box::new(instantiate_module_state(
                hir,
                child,
                child_ports,
                objects,
                stack,
            )?),
        });
    }

    stack.pop();
    let net_specials = build_net_special_table(module, &signals, objects);
    let child_output_driven = build_child_output_driven_table(&children);
    Ok(ModuleState {
        module_name: elaborated.module_name.clone(),
        parameter_values,
        signals,
        memories: build_memory_table(module),
        previous_clocks: build_clock_state_table(module),
        legacy_rom: build_legacy_rom_state(hir, module)?,
        children,
        net_specials,
        child_output_driven,
    })
}

fn build_net_special_table(
    module: &ModuleSummary,
    signals: &FxHashMap<String, SignalBinding>,
    objects: &[RuntimeObjectLayout],
) -> FxHashMap<String, (SignalBinding, NetSpecialAction)> {
    let mut specials = FxHashMap::default();

    for port in &module.ports {
        let Some(binding) = signals.get(&port.name).copied() else {
            continue;
        };
        if !objects
            .get(binding.object_id)
            .is_some_and(|object| object.storage.is_net())
        {
            continue;
        }
        if matches!(port.direction, PortDirection::Output) && port.storage.is_variable() {
            specials.insert(
                port.name.clone(),
                (binding, NetSpecialAction::ReplaceDriver),
            );
        } else if signal_has_procedural_driver(module, &port.name) {
            specials.insert(
                port.name.clone(),
                (binding, NetSpecialAction::StageIfUndriven),
            );
        }
    }

    for signal in &module.signals {
        let Some(binding) = signals.get(&signal.name).copied() else {
            continue;
        };
        if !objects
            .get(binding.object_id)
            .is_some_and(|object| object.storage.is_net())
        {
            continue;
        }
        if signal.storage.is_variable() {
            specials.insert(
                signal.name.clone(),
                (binding, NetSpecialAction::ReplaceDriver),
            );
        } else if signal_has_procedural_driver(module, &signal.name) {
            specials.insert(
                signal.name.clone(),
                (binding, NetSpecialAction::StageIfUndriven),
            );
        }
    }

    specials
}

fn build_child_output_driven_table(children: &[ChildState]) -> FxHashSet<String> {
    let mut driven = FxHashSet::default();
    for child in children {
        for sink in &child.output_sinks {
            if let LValue::Signal(name) = &sink.target {
                driven.insert(name.clone());
            }
        }
    }
    driven
}

/// Per-pass replacement for `sync_instance_values_to_frame` in the settle
/// loop: applies the same per-signal policy, but only to the names the pass
/// actually wrote (`overlay`) plus the precomputed `net_specials` that need
/// their drivers refreshed every pass. Net-storage signals are staged, not
/// written (`write_net_values = false` semantics); signals wholly driven by
/// a child output are skipped (`defer_child_output_net_drivers = true`
/// semantics).
pub(super) fn commit_overlay_to_frame(
    state: &ModuleState,
    overlay: &FxHashMap<String, Value>,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
    let mut changed = false;

    for (name, value) in overlay {
        if state.child_output_driven.contains(name) {
            continue;
        }
        let Some(binding) = state.signals.get(name).copied() else {
            // Overlay entries that are not signals (parameter-named seeds)
            // have no frame slot, matching the old sync which only walked
            // declared ports and signals.
            continue;
        };
        if object_layouts
            .get(binding.object_id)
            .is_some_and(|object| object.storage.is_net())
        {
            match state.net_specials.get(name) {
                Some((_, NetSpecialAction::ReplaceDriver)) => {
                    replace_whole_signal_driver(
                        binding,
                        value.clone(),
                        object_layouts,
                        net_drivers,
                    )?;
                    changed |= write_binding(binding, value.clone(), frame, object_layouts)?;
                }
                Some((_, NetSpecialAction::StageIfUndriven)) => {
                    if !net_drivers.contains_key(&binding.object_id) {
                        stage_whole_signal_driver(
                            binding,
                            value.clone(),
                            object_layouts,
                            net_drivers,
                        )?;
                    }
                }
                None => {}
            }
            continue;
        }
        changed |= write_binding(binding, value.clone(), frame, object_layouts)?;
    }

    for (name, (binding, action)) in &state.net_specials {
        if overlay.contains_key(name) || state.child_output_driven.contains(name) {
            continue;
        }
        let value = read_binding(*binding, frame)?;
        match action {
            NetSpecialAction::ReplaceDriver => {
                replace_whole_signal_driver(*binding, value, object_layouts, net_drivers)?;
            }
            NetSpecialAction::StageIfUndriven => {
                if !net_drivers.contains_key(&binding.object_id) {
                    stage_whole_signal_driver(*binding, value, object_layouts, net_drivers)?;
                }
            }
        }
    }

    Ok(changed)
}

pub(super) fn runtime_bits_width(shape: RuntimeObjectShape) -> Result<usize> {
    match shape {
        RuntimeObjectShape::Bits { width } => Ok(width),
        RuntimeObjectShape::Memory { .. } => Err(Error::Unsupported(
            "memory shapes are not valid for scalar runtime objects".into(),
        )),
    }
}

pub(super) fn allocate_runtime_object(
    objects: &mut Vec<RuntimeObjectLayout>,
    width: usize,
    storage: StorageKind,
) -> SignalBinding {
    let object_id = objects.len();
    objects.push(RuntimeObjectLayout {
        width: width.max(1),
        storage,
    });
    SignalBinding {
        object_id,
        view_width: width.max(1),
    }
}

pub(super) fn grow_runtime_object(
    objects: &mut [RuntimeObjectLayout],
    object_id: usize,
    width: usize,
) {
    if let Some(object) = objects.get_mut(object_id) {
        object.width = object.width.max(width.max(1));
    }
}

pub(super) fn aliasable_parent_signal_name<'a>(
    parent_module: &'a ModuleSummary,
    direction: PortDirection,
    target: Option<&'a LValue>,
) -> Option<&'a str> {
    let LValue::Signal(name) = target? else {
        return None;
    };

    match direction {
        PortDirection::Input => Some(name.as_str()),
        PortDirection::Output | PortDirection::Inout
            if signal_storage(parent_module, name).is_some_and(StorageKind::is_net) =>
        {
            Some(name.as_str())
        }
        PortDirection::Output | PortDirection::Inout | PortDirection::Ref => None,
    }
}

pub(super) fn signal_storage(module: &ModuleSummary, name: &str) -> Option<StorageKind> {
    module
        .port(name)
        .map(|port| port.storage)
        .or_else(|| module.signal_decl(name).map(|signal| signal.storage))
}

/// Frame-backed `ValueReader`: reads signals straight from the indexed
/// runtime frame and parameters from the instance's resolved values,
/// mirroring the lookup precedence of `build_instance_value_table`
/// (parameters shadow same-named signals).
pub(super) struct FrameValues<'a> {
    pub(super) module: &'a ModuleSummary,
    pub(super) state: &'a ModuleState,
    pub(super) frame: &'a [ObjectValue],
}

impl ValueReader for FrameValues<'_> {
    fn read_value(&self, name: &str) -> Option<Value> {
        if let Some(param) = self.module.parameter_decl(name) {
            return Some(
                self.state
                    .parameter_values
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| Value::zero(param.width()))
                    .coerced_to(param.width()),
            );
        }
        let binding = self.state.signals.get(name).copied()?;
        read_binding(binding, self.frame).ok()
    }
}

/// `FrameValues` plus a copy-on-write overlay of the values the current
/// settle pass has written. Reads prefer the overlay so blocking-assignment
/// effects stay visible within the pass, exactly as the old full value
/// table behaved after in-place mutation.
pub(super) struct OverlayValues<'a> {
    pub(super) module: &'a ModuleSummary,
    pub(super) state: &'a ModuleState,
    pub(super) frame: &'a [ObjectValue],
    pub(super) overlay: &'a FxHashMap<String, Value>,
}

impl ValueReader for OverlayValues<'_> {
    fn read_value(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.overlay.get(name) {
            return Some(value.clone());
        }
        FrameValues {
            module: self.module,
            state: self.state,
            frame: self.frame,
        }
        .read_value(name)
    }
}

pub(super) fn read_binding(binding: SignalBinding, values: &[ObjectValue]) -> Result<Value> {
    let logic = read_binding_logic(binding, values)?;
    Ok(Value::from_logic(logic, binding.view_width))
}

pub(super) fn read_binding_logic(
    binding: SignalBinding,
    values: &[ObjectValue],
) -> Result<LogicValue> {
    let value = values.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    Ok(value.logic.coerced_to(binding.view_width))
}

pub(super) fn write_binding(
    binding: SignalBinding,
    value: Value,
    values: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<bool> {
    let logic = value.coerced_to(binding.view_width).logic;
    write_binding_logic(binding, logic, values, object_layouts)
}

pub(super) fn write_binding_logic(
    binding: SignalBinding,
    value: LogicValue,
    values: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<bool> {
    let object = object_layouts.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    let current = values.get_mut(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} has no value slot",
            binding.object_id
        ))
    })?;
    let next = ObjectValue::from_logic(
        value
            .coerced_to(binding.view_width)
            .coerced_to(object.width),
    );
    let changed = *current != next;
    *current = next;
    Ok(changed)
}

pub(super) fn seed_runtime_frame(
    module: &ModuleSummary,
    state: &ModuleState,
    persisted: &[ObjectValue],
    _object_layouts: &[RuntimeObjectLayout],
    inputs: &BTreeMap<String, LogicValue>,
) -> Result<Vec<ObjectValue>> {
    for name in inputs.keys() {
        if module.port(name).is_none() {
            return Err(Error::Resolve(format!(
                "input '{}' does not match any port on module '{}'",
                name, module.name
            )));
        }
    }

    let _ = state;
    Ok(persisted.to_vec())
}

pub(super) fn build_instance_value_table(
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[ObjectValue],
) -> Result<FxHashMap<String, Value>> {
    let mut values = FxHashMap::default();

    for port in &module.ports {
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        values.insert(port.name.clone(), read_binding(binding, frame)?);
    }

    for signal in &module.signals {
        let binding = state.signals.get(&signal.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                signal.name, module.name
            ))
        })?;
        values.insert(signal.name.clone(), read_binding(binding, frame)?);
    }

    for param in &module.parameters {
        let coerced = state
            .parameter_values
            .get(&param.name)
            .cloned()
            .unwrap_or_else(|| Value::zero(param.width()))
            .coerced_to(param.width());
        values.insert(param.name.clone(), coerced);
    }

    Ok(values)
}

pub(super) fn sync_instance_values_to_frame(
    module: &ModuleSummary,
    state: &ModuleState,
    values: &FxHashMap<String, Value>,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
    write_net_values: bool,
    defer_child_output_net_drivers: bool,
) -> Result<bool> {
    let mut changed = false;

    for port in &module.ports {
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        let value = values
            .get(&port.name)
            .cloned()
            .unwrap_or_else(|| Value::zero(port.width()));
        if defer_child_output_net_drivers && signal_has_whole_child_output_driver(state, &port.name)
        {
            continue;
        }
        if object_layouts
            .get(binding.object_id)
            .is_some_and(|object| object.storage.is_net())
        {
            if matches!(port.direction, PortDirection::Output) && port.storage.is_variable() {
                replace_whole_signal_driver(binding, value.clone(), object_layouts, net_drivers)?;
                changed |= write_binding(binding, value, frame, object_layouts)?;
                continue;
            }
            if net_drivers.contains_key(&binding.object_id) {
                continue;
            }
            if signal_has_procedural_driver(module, &port.name) {
                stage_whole_signal_driver(binding, value.clone(), object_layouts, net_drivers)?;
                if write_net_values {
                    changed |= write_binding(binding, value, frame, object_layouts)?;
                }
            }
            continue;
        }
        changed |= write_binding(binding, value, frame, object_layouts)?;
    }

    for signal in &module.signals {
        let binding = state.signals.get(&signal.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                signal.name, module.name
            ))
        })?;
        let value = values
            .get(&signal.name)
            .cloned()
            .unwrap_or_else(|| Value::zero(signal.width()));
        if defer_child_output_net_drivers
            && signal_has_whole_child_output_driver(state, &signal.name)
        {
            continue;
        }
        if object_layouts
            .get(binding.object_id)
            .is_some_and(|object| object.storage.is_net())
        {
            if signal.storage.is_variable() {
                replace_whole_signal_driver(binding, value.clone(), object_layouts, net_drivers)?;
                changed |= write_binding(binding, value, frame, object_layouts)?;
                continue;
            }
            if net_drivers.contains_key(&binding.object_id) {
                continue;
            }
            if signal_has_procedural_driver(module, &signal.name) {
                stage_whole_signal_driver(binding, value.clone(), object_layouts, net_drivers)?;
                if write_net_values {
                    changed |= write_binding(binding, value, frame, object_layouts)?;
                }
            }
            continue;
        }
        changed |= write_binding(binding, value, frame, object_layouts)?;
    }

    Ok(changed)
}

pub(super) fn signal_has_procedural_driver(module: &ModuleSummary, signal_name: &str) -> bool {
    module
        .proc_blocks
        .iter()
        .any(|block| stmt_writes_signal(&block.body, signal_name))
}

pub(super) fn signal_has_whole_child_output_driver(state: &ModuleState, signal_name: &str) -> bool {
    state.children.iter().any(|child| {
        child
            .output_sinks
            .iter()
            .any(|sink| matches!(&sink.target, LValue::Signal(name) if name == signal_name))
    })
}

pub(super) fn stmt_writes_signal(stmt: &Stmt, signal_name: &str) -> bool {
    match stmt {
        Stmt::Empty => false,
        Stmt::Block(statements) => statements
            .iter()
            .any(|statement| stmt_writes_signal(statement, signal_name)),
        Stmt::Assign { target, .. } => lvalue_contains_signal(target, signal_name),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_writes_signal(then_branch, signal_name)
                || else_branch
                    .as_ref()
                    .is_some_and(|else_branch| stmt_writes_signal(else_branch, signal_name))
        }
        Stmt::Case { items, default, .. } => {
            items
                .iter()
                .any(|item| stmt_writes_signal(&item.body, signal_name))
                || default
                    .as_ref()
                    .is_some_and(|default| stmt_writes_signal(default, signal_name))
        }
    }
}

pub(super) fn lvalue_contains_signal(lvalue: &LValue, signal_name: &str) -> bool {
    match lvalue {
        LValue::Signal(name) => name == signal_name,
        LValue::Concat(items) => items
            .iter()
            .any(|item| lvalue_contains_signal(item, signal_name)),
        LValue::BitSelect { signal, .. } | LValue::PartSelect { signal, .. } => {
            signal == signal_name
        }
        LValue::MemoryElement { .. } => false,
    }
}

pub(super) fn stage_object_driver(
    object_id: usize,
    value: LogicValue,
    net_drivers: &mut NetDriverTable,
) {
    net_drivers
        .entry(object_id)
        .or_default()
        .push(NetDriver::new(value, DriveStrengthPair::STRONG));
}

pub(super) fn replace_object_driver(
    object_id: usize,
    value: LogicValue,
    net_drivers: &mut NetDriverTable,
) {
    net_drivers.insert(
        object_id,
        vec![NetDriver::new(value, DriveStrengthPair::STRONG)],
    );
}

pub(super) fn replace_whole_signal_driver(
    binding: SignalBinding,
    value: Value,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let object = object_layouts.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    let logic = value
        .coerced_to(binding.view_width)
        .logic
        .coerced_to(object.width);
    replace_object_driver(binding.object_id, logic, net_drivers);
    Ok(())
}

pub(super) fn build_clock_state_table(module: &ModuleSummary) -> FxHashMap<String, bool> {
    let mut clocks = FxHashMap::default();

    for block in &module.proc_blocks {
        if let ProcBlockKind::AlwaysFf { clock, async_reset } = &block.kind {
            clocks.entry(clock.clone()).or_insert(false);
            if let Some(async_reset) = async_reset {
                clocks.entry(async_reset.clone()).or_insert(false);
            }
        }
    }

    clocks
}

pub(super) fn resolve_instance_path<'a>(
    state: &'a ModuleState,
    instance_path: &[&str],
) -> Result<&'a ModuleState> {
    let Some((segment, rest)) = instance_path.split_first() else {
        return Ok(state);
    };
    let child_index = state
        .children
        .iter()
        .position(|child| child.instance_name == *segment)
        .ok_or_else(|| {
            Error::Resolve(format!(
                "instance path '{}' does not exist under module '{}'",
                instance_path.join("."),
                state.module_name
            ))
        })?;
    resolve_instance_path(state.children[child_index].state.as_ref(), rest)
}

pub(super) fn resolve_instance_path_mut<'a>(
    state: &'a mut ModuleState,
    instance_path: &[&str],
) -> Result<&'a mut ModuleState> {
    let Some((segment, rest)) = instance_path.split_first() else {
        return Ok(state);
    };
    let child_index = state
        .children
        .iter()
        .position(|child| child.instance_name == *segment)
        .ok_or_else(|| {
            Error::Resolve(format!(
                "instance path '{}' does not exist under module '{}'",
                instance_path.join("."),
                state.module_name
            ))
        })?;
    resolve_instance_path_mut(state.children[child_index].state.as_mut(), rest)
}
