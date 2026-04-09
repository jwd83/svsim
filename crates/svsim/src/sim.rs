use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use crate::bit_value::{BitValue, ParseBitValueError};
use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::elaborate::{ElaboratedInstance, RuntimeObjectShape};
use crate::hir::{
    AssignmentKind, BinaryOp, Expr, HirDesign, LValue, ModuleInstanceSummary, ModuleSummary,
    NumericLiteral, PackedRange, PortDirection, ProcBlockKind, Stmt, StorageKind, UnaryOp,
};
use crate::validate::resolve_legacy_rom_data_path;
use crate::width::{
    arithmetic_shift_right_bits, expr_width, mask, minimum_width, shift_left_bits,
    shift_right_bits, sign_extend_bits,
};

#[derive(Debug, Clone)]
pub struct SimulationSession {
    design: CompiledDesign,
    objects: Vec<RuntimeObjectLayout>,
    persisted: Vec<Value>,
    state: ModuleState,
}

#[derive(Debug, Clone)]
struct ModuleState {
    module_name: String,
    parameter_values: HashMap<String, Value>,
    signals: HashMap<String, SignalBinding>,
    memories: HashMap<String, MemoryState>,
    previous_clocks: HashMap<String, bool>,
    legacy_rom: Option<LegacyRomState>,
    children: Vec<ChildState>,
}

#[derive(Debug, Clone)]
struct ChildState {
    instance_name: String,
    input_drivers: Vec<PortExprDriver>,
    output_sinks: Vec<PortSink>,
    state: Box<ModuleState>,
}

#[derive(Debug, Clone)]
struct InstanceEvalCache {
    inputs: BTreeMap<String, BitValue>,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeObjectLayout {
    width: usize,
    _storage: StorageKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SignalBinding {
    object_id: usize,
    view_width: usize,
}

impl SignalBinding {
    fn with_view_width(self, view_width: usize) -> Self {
        Self {
            object_id: self.object_id,
            view_width: view_width.max(1),
        }
    }
}

#[derive(Debug, Clone)]
struct PortExprDriver {
    port_name: String,
    expr: Expr,
}

#[derive(Debug, Clone)]
struct PortSink {
    port_name: String,
    target: LValue,
}

#[derive(Debug, Clone)]
struct MemoryState {
    index_range: PackedRange,
    words: Vec<Value>,
}

#[derive(Debug, Clone)]
struct LegacyRomState {
    addr_port: String,
    data_port: String,
    words: Vec<Value>,
}

impl MemoryState {
    fn read(&self, index: usize, memory_name: &str) -> Result<Value> {
        let offset = self.index_range.index_offset(index).ok_or_else(|| {
            Error::Resolve(format!(
                "memory index [{}] is out of range for '{}'",
                index, memory_name
            ))
        })?;
        Ok(self.words[offset].clone())
    }

    fn write(&mut self, index: usize, value: Value, memory_name: &str) -> Result<bool> {
        let offset = self.index_range.index_offset(index).ok_or_else(|| {
            Error::Resolve(format!(
                "memory index [{}] is out of range for '{}'",
                index, memory_name
            ))
        })?;
        let current = self
            .words
            .get_mut(offset)
            .expect("memory offset is guaranteed to be in range");
        let next = value.coerced_to(current.width);
        let changed = *current != next;
        *current = next;
        Ok(changed)
    }
}

impl SimulationSession {
    pub(crate) fn new(design: CompiledDesign) -> Result<Self> {
        let elaborated = design.elaborate()?;
        let mut objects = Vec::new();
        let mut stack = Vec::new();
        let state = instantiate_module_state(
            design.hir(),
            &elaborated.top,
            HashMap::new(),
            None,
            None,
            &mut objects,
            &mut stack,
        )?;
        let persisted = objects
            .iter()
            .map(|object| Value::zero(object.width))
            .collect();
        Ok(Self {
            design,
            objects,
            persisted,
            state,
        })
    }

    pub fn top_module(&self) -> &str {
        self.design
            .top_module()
            .expect("compiled designs always carry a top module")
    }

    pub fn load_memory_words(
        &mut self,
        instance_path: &[&str],
        memory_name: &str,
        words: &[BitValue],
    ) -> Result<()> {
        let hir = self.design.hir();
        let module_state = resolve_instance_path_mut(&mut self.state, instance_path)?;
        let module = resolve_supported_module(hir, &module_state.module_name)?;
        let memory_decl = module.memory_decl(memory_name).ok_or_else(|| {
            Error::Resolve(format!(
                "memory '{}' is not declared in '{}'",
                memory_name, module.name
            ))
        })?;
        let memory_state = module_state.memories.get_mut(memory_name).ok_or_else(|| {
            Error::Resolve(format!(
                "memory '{}' has no runtime storage in '{}'",
                memory_name, module.name
            ))
        })?;

        for (offset, word) in words.iter().enumerate() {
            let index = memory_decl.index_range.low() + offset;
            memory_state.write(
                index,
                Value::new(word.clone(), memory_decl.element_width()),
                memory_name,
            )?;
        }

        Ok(())
    }

    pub fn load_memory_file(
        &mut self,
        instance_path: &[&str],
        memory_name: &str,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let path = path.as_ref();
        let hir = self.design.hir();
        let module_state = resolve_instance_path_mut(&mut self.state, instance_path)?;
        let module = resolve_supported_module(hir, &module_state.module_name)?;
        let memory_decl = module.memory_decl(memory_name).ok_or_else(|| {
            Error::Resolve(format!(
                "memory '{}' is not declared in '{}'",
                memory_name, module.name
            ))
        })?;
        let writes = parse_memory_file(path, memory_decl.element_width(), memory_decl.depth())?;
        let memory_state = module_state.memories.get_mut(memory_name).ok_or_else(|| {
            Error::Resolve(format!(
                "memory '{}' has no runtime storage in '{}'",
                memory_name, module.name
            ))
        })?;

        for (index, word) in writes {
            memory_state.write(
                index,
                Value::new(word, memory_decl.element_width()),
                memory_name,
            )?;
        }

        Ok(())
    }

    pub fn read_memory_word(
        &self,
        instance_path: &[&str],
        memory_name: &str,
        index: usize,
    ) -> Result<BitValue> {
        let hir = self.design.hir();
        let module_state = resolve_instance_path(&self.state, instance_path)?;
        let module = resolve_supported_module(hir, &module_state.module_name)?;
        if module.memory_decl(memory_name).is_none() {
            return Err(Error::Resolve(format!(
                "memory '{}' is not declared in '{}'",
                memory_name, module.name
            )));
        }
        let memory_state = module_state.memories.get(memory_name).ok_or_else(|| {
            Error::Resolve(format!(
                "memory '{}' has no runtime storage in '{}'",
                memory_name, module.name
            ))
        })?;
        Ok(memory_state.read(index, memory_name)?.normalized_bits())
    }

    pub fn read_signal(
        &self,
        inputs: &BTreeMap<String, BitValue>,
        instance_path: &[&str],
        signal_name: &str,
    ) -> Result<BitValue> {
        let hir = self.design.hir();
        let module = top_module(hir, self.top_module())?;
        let mut frame =
            seed_runtime_frame(module, &self.state, &self.persisted, &self.objects, inputs)?;
        let mut stack = Vec::new();
        settle_module(hir, module, &self.state, &mut frame, &self.objects, &mut stack)?;

        let module_state = resolve_instance_path(&self.state, instance_path)?;
        let instance_module = resolve_supported_module(hir, &module_state.module_name)?;
        let values = build_instance_value_table(instance_module, module_state, &frame, &self.objects)?;
        values
            .get(signal_name)
            .cloned()
            .map(|value| value.normalized_bits())
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal_name, instance_module.name
                ))
            })
    }

    pub fn eval_once(
        &mut self,
        inputs: BTreeMap<String, BitValue>,
    ) -> Result<BTreeMap<String, BitValue>> {
        let module = top_module(self.design.hir(), self.top_module())?;
        let mut frame = seed_runtime_frame(
            module,
            &self.state,
            &self.persisted,
            &self.objects,
            &inputs,
        )?;
        let mut stack = Vec::new();
        settle_module(
            self.design.hir(),
            module,
            &self.state,
            &mut frame,
            &self.objects,
            &mut stack,
        )?;
        Ok(collect_outputs(module, &self.state, &frame, &self.objects))
    }

    pub fn step(
        &mut self,
        inputs: BTreeMap<String, BitValue>,
    ) -> Result<BTreeMap<String, BitValue>> {
        let hir = self.design.hir();
        let module = top_module(hir, self.top_module())?;
        let mut pre_frame =
            seed_runtime_frame(module, &self.state, &self.persisted, &self.objects, &inputs)?;
        let mut settle_stack = Vec::new();
        settle_module(
            hir,
            module,
            &self.state,
            &mut pre_frame,
            &self.objects,
            &mut settle_stack,
        )?;

        let mut next_persisted = self.persisted.clone();
        let mut step_stack = Vec::new();
        step_module(
            hir,
            module,
            &mut self.state,
            &pre_frame,
            &mut next_persisted,
            &self.objects,
            &mut step_stack,
        )?;
        self.persisted = next_persisted;

        let mut post_frame =
            seed_runtime_frame(module, &self.state, &self.persisted, &self.objects, &inputs)?;
        let mut post_settle_stack = Vec::new();
        settle_module(
            hir,
            module,
            &self.state,
            &mut post_frame,
            &self.objects,
            &mut post_settle_stack,
        )?;
        Ok(collect_outputs(
            module,
            &self.state,
            &post_frame,
            &self.objects,
        ))
    }
}

fn parse_memory_file(
    path: &Path,
    word_width: usize,
    depth: usize,
) -> Result<Vec<(usize, BitValue)>> {
    let text = fs::read_to_string(path)?;
    parse_memory_text(&text, path, word_width, depth)
}

fn parse_memory_text(
    text: &str,
    path: &Path,
    word_width: usize,
    depth: usize,
) -> Result<Vec<(usize, BitValue)>> {
    let mut writes = Vec::new();
    let mut current_address = 0usize;

    for (line_number, raw_line) in text.lines().enumerate() {
        let Some(line) = strip_memory_comments(raw_line) else {
            continue;
        };

        let (address, value_text) = if let Some((address_text, value_text)) = line.split_once(':') {
            let address = parse_memory_address(address_text, path, line_number + 1)?;
            (address, value_text.trim())
        } else {
            (current_address, line)
        };

        if address >= depth {
            return Err(Error::Parse(format!(
                "memory file '{}' line {} writes address {} outside depth {}",
                path.display(),
                line_number + 1,
                address,
                depth
            )));
        }

        let value = parse_memory_value(value_text, path, line_number + 1)?;
        writes.push((address, value.truncate(word_width)));
        current_address = address + 1;
    }

    Ok(writes)
}

fn strip_memory_comments(line: &str) -> Option<&str> {
    let mut end = line.len();
    for marker in ["//", "#"] {
        if let Some(index) = line.find(marker) {
            end = end.min(index);
        }
    }

    let trimmed = line[..end].trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn parse_memory_address(text: &str, path: &Path, line_number: usize) -> Result<usize> {
    let raw = text.trim().replace('_', "");
    if let Ok(value) = parse_prefixed_value(&raw) {
        value.to_usize_checked().ok_or_else(|| {
            Error::Parse(format!(
                "memory file '{}' line {} has an address too large for this host",
                path.display(),
                line_number
            ))
        })
    } else {
        Err(Error::Parse(format!(
            "memory file '{}' line {} has an invalid address '{}'",
            path.display(),
            line_number,
            text.trim()
        )))
    }
}

fn parse_memory_value(text: &str, path: &Path, line_number: usize) -> Result<BitValue> {
    let raw = text.trim().replace('_', "");
    if raw.chars().all(|ch| matches!(ch, '0' | '1')) {
        return BitValue::from_str_radix(&raw, 2).map_err(|_| {
            Error::Parse(format!(
                "memory file '{}' line {} has an invalid binary value '{}'",
                path.display(),
                line_number,
                text.trim()
            ))
        });
    }

    parse_prefixed_value(&raw).map_err(|_| {
        Error::Parse(format!(
            "memory file '{}' line {} has an invalid value '{}'",
            path.display(),
            line_number,
            text.trim()
        ))
    })
}

fn parse_prefixed_value(raw: &str) -> std::result::Result<BitValue, ParseBitValueError> {
    if let Some(rest) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        BitValue::from_str_radix(rest, 16)
    } else if let Some(rest) = raw.strip_prefix("0b").or_else(|| raw.strip_prefix("0B")) {
        BitValue::from_str_radix(rest, 2)
    } else if let Some(rest) = raw.strip_prefix("0o").or_else(|| raw.strip_prefix("0O")) {
        BitValue::from_str_radix(rest, 8)
    } else {
        raw.parse()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Value {
    bits: BitValue,
    width: usize,
    signed: bool,
}

impl Value {
    fn new(bits: BitValue, width: usize) -> Self {
        Self::new_with_signed(bits, width, false)
    }

    fn new_with_signed(bits: BitValue, width: usize, signed: bool) -> Self {
        let width = width.max(1);
        Self {
            bits: bits.truncate(width),
            width,
            signed,
        }
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

    fn zero(width: usize) -> Self {
        Self::new(BitValue::zero(), width)
    }

    fn normalized_bits(&self) -> BitValue {
        self.bits.clone()
    }

    fn truthy(&self) -> bool {
        !self.bits.is_zero()
    }
}

fn top_module<'a>(hir: &'a HirDesign, module_name: &str) -> Result<&'a ModuleSummary> {
    resolve_supported_module(hir, module_name)
}

fn instantiate_module_state(
    hir: &HirDesign,
    elaborated: &ElaboratedInstance,
    provided_ports: HashMap<String, SignalBinding>,
    parent_module: Option<&ModuleSummary>,
    parent_parameter_values: Option<&HashMap<String, Value>>,
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
    let instance_summary = match (parent_module, elaborated.instance_name.as_deref()) {
        (Some(parent_module), Some(instance_name)) => Some(
            parent_module
                .instantiations
                .iter()
                .find(|instance| instance.instance_name == instance_name)
                .ok_or_else(|| {
                    Error::Resolve(format!(
                        "instance '{}' does not exist under '{}'",
                        instance_name, parent_module.name
                    ))
                })?,
        ),
        _ => None,
    };
    let parameter_values = elaborate_module_parameters(
        module,
        parent_module,
        parent_parameter_values,
        instance_summary,
    )?;

    stack.push(elaborated.module_name.clone());

    let mut signals = HashMap::new();
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
        let mut child_ports = HashMap::new();
        let mut input_drivers = Vec::new();
        let mut output_sinks = Vec::new();

        for port in &child.ports {
            let port_width = runtime_bits_width(port.shape)?;
            let binding = if let Some(binding) = child.bindings.iter().find(|binding| binding.port_name == port.name) {
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
                    let binding_slot =
                        allocate_runtime_object(objects, port_width, port.storage);
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
                Some(module),
                Some(&parameter_values),
                objects,
                stack,
            )?),
        });
    }

    stack.pop();
    Ok(ModuleState {
        module_name: elaborated.module_name.clone(),
        parameter_values,
        signals,
        memories: build_memory_table(module),
        previous_clocks: build_clock_state_table(module),
        legacy_rom: build_legacy_rom_state(hir, module)?,
        children,
    })
}

fn settle_module(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &mut [Value],
    object_layouts: &[RuntimeObjectLayout],
    stack: &mut Vec<String>,
) -> Result<()> {
    if stack.iter().any(|name| name == &state.module_name) {
        return Err(Error::Unsupported(format!(
            "recursive combinational instantiation detected at {} -> {}",
            stack.join(" -> "),
            state.module_name
        )));
    }

    let max_iterations = ((module.continuous_assignments.len()
        + module.proc_blocks.len()
        + state.children.len()
        + state.signals.len())
    .max(1))
        * 8;
    let mut instance_caches: Vec<Option<InstanceEvalCache>> = vec![None; state.children.len()];

    stack.push(state.module_name.clone());
    let mut converged = false;
    for _ in 0..max_iterations {
        let mut values = build_instance_value_table(module, state, frame, object_layouts)?;
        let mut changed = false;

        if let Some(legacy_rom) = &state.legacy_rom {
            apply_legacy_rom_outputs(module, &mut values, legacy_rom)?;
            changed |= sync_instance_values_to_frame(module, state, &values, frame, object_layouts)?;
        } else {
            for assign in &module.continuous_assignments {
                let value = eval_expr(&assign.expr, module, &values, &state.memories)?;
                let target = resolve_lvalue(&assign.target, module, &values, &state.memories)?;
                if resolved_lvalue_contains_memory(&target) {
                    return Err(Error::Unsupported(
                        "continuous assignments to memory elements are not supported".into(),
                    ));
                }
                let mut no_memories = HashMap::new();
                changed |=
                    apply_resolved_lvalue(&target, value, module, &mut values, &mut no_memories)?;
            }

            for block in &module.proc_blocks {
                changed |= execute_proc_block(
                    &block.kind,
                    &block.body,
                    module,
                    &mut values,
                    &state.memories,
                )?;
            }

            changed |= sync_instance_values_to_frame(module, state, &values, frame, object_layouts)?;

            for (child_state, cache) in state.children.iter().zip(instance_caches.iter_mut()) {
                let child = resolve_supported_module(hir, &child_state.state.module_name)?;
                let parent_values =
                    build_instance_value_table(module, state, frame, object_layouts)?;
                changed |= drive_child_inputs(
                    module,
                    child_state,
                    &parent_values,
                    &state.memories,
                    frame,
                    object_layouts,
                )?;

                let child_inputs =
                    snapshot_child_inputs(child, child_state.state.as_ref(), frame, object_layouts)?;
                let needs_refresh = cache
                    .as_ref()
                    .is_none_or(|cached| cached.inputs != child_inputs);
                if needs_refresh {
                    settle_module(
                        hir,
                        child,
                        child_state.state.as_ref(),
                        frame,
                        object_layouts,
                        stack,
                    )?;
                    *cache = Some(InstanceEvalCache { inputs: child_inputs });
                    changed = true;
                }

                let parent_values =
                    build_instance_value_table(module, state, frame, object_layouts)?;
                changed |= apply_child_output_sinks(
                    module,
                    state,
                    child_state,
                    &parent_values,
                    &state.memories,
                    frame,
                    object_layouts,
                )?;
            }
        }

        if !changed {
            converged = true;
            break;
        }
    }
    stack.pop();

    if !converged {
        return Err(Error::Unsupported(format!(
            "combinational evaluation did not converge for module '{}'",
            module.name
        )));
    }

    Ok(())
}

fn step_module(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &mut ModuleState,
    pre_frame: &[Value],
    next_objects: &mut [Value],
    object_layouts: &[RuntimeObjectLayout],
    _stack: &mut Vec<String>,
) -> Result<()> {
    let pre_values = build_instance_value_table(module, state, pre_frame, object_layouts)?;

    for child_state in &mut state.children {
        let child = resolve_supported_module(hir, &child_state.state.module_name)?;
        step_module(
            hir,
            child,
            child_state.state.as_mut(),
            pre_frame,
            next_objects,
            object_layouts,
            _stack,
        )?;
    }

    let mut staged = build_instance_value_table(module, state, next_objects, object_layouts)?;
    let mut staged_memories = state.memories.clone();
    let mut sampled_clocks = state.previous_clocks.clone();
    for block in &module.proc_blocks {
        match &block.kind {
            ProcBlockKind::AlwaysComb => {}
            ProcBlockKind::AlwaysFf { clock, async_reset } => {
                let clock_value = pre_values.get(clock).cloned().ok_or_else(|| {
                    Error::Resolve(format!(
                        "clock '{}' is not declared in '{}'",
                        clock, module.name
                    ))
                })?;
                let previous_clock_value =
                    state.previous_clocks.get(clock).copied().unwrap_or(false);
                let current_clock_value = clock_value.truthy();
                let reset_edge = if let Some(async_reset) = async_reset {
                    let reset_value = pre_values.get(async_reset).cloned().ok_or_else(|| {
                        Error::Resolve(format!(
                            "async reset '{}' is not declared in '{}'",
                            async_reset, module.name
                        ))
                    })?;
                    let previous_reset_value = state
                        .previous_clocks
                        .get(async_reset)
                        .copied()
                        .unwrap_or(false);
                    let current_reset_value = reset_value.truthy();
                    sampled_clocks.insert(async_reset.clone(), current_reset_value);
                    !previous_reset_value && current_reset_value
                } else {
                    false
                };
                if (!previous_clock_value && current_clock_value) || reset_edge {
                    let mut exec_values = pre_values.clone();
                    let mut exec_memories = state.memories.clone();
                    let mut block_staged = staged.clone();
                    let mut block_staged_memories = staged_memories.clone();
                    execute_sequential_stmt(
                        &block.body,
                        module,
                        &mut exec_values,
                        &mut exec_memories,
                        &mut block_staged,
                        &mut block_staged_memories,
                    )?;
                    staged = block_staged;
                    staged_memories = block_staged_memories;
                }
                sampled_clocks.insert(clock.clone(), current_clock_value);
            }
        }
    }

    sync_instance_values_to_frame(module, state, &staged, next_objects, object_layouts)?;
    state.memories = staged_memories;
    state.previous_clocks = sampled_clocks;
    Ok(())
}

fn runtime_bits_width(shape: RuntimeObjectShape) -> Result<usize> {
    match shape {
        RuntimeObjectShape::Bits { width } => Ok(width),
        RuntimeObjectShape::Memory { .. } => Err(Error::Unsupported(
            "memory shapes are not valid for scalar runtime objects".into(),
        )),
    }
}

fn allocate_runtime_object(
    objects: &mut Vec<RuntimeObjectLayout>,
    width: usize,
    storage: StorageKind,
) -> SignalBinding {
    let object_id = objects.len();
    objects.push(RuntimeObjectLayout {
        width: width.max(1),
        _storage: storage,
    });
    SignalBinding {
        object_id,
        view_width: width.max(1),
    }
}

fn grow_runtime_object(objects: &mut [RuntimeObjectLayout], object_id: usize, width: usize) {
    if let Some(object) = objects.get_mut(object_id) {
        object.width = object.width.max(width.max(1));
    }
}

fn aliasable_parent_signal_name<'a>(
    parent_module: &'a ModuleSummary,
    direction: PortDirection,
    target: Option<&'a LValue>,
) -> Option<&'a str> {
    let LValue::Signal(name) = target? else {
        return None;
    };

    match direction {
        PortDirection::Input => Some(name.as_str()),
        PortDirection::Output if signal_storage(parent_module, name).is_some_and(StorageKind::is_net) => {
            Some(name.as_str())
        }
        PortDirection::Output | PortDirection::Inout | PortDirection::Ref => None,
    }
}

fn signal_storage(module: &ModuleSummary, name: &str) -> Option<StorageKind> {
    module
        .port(name)
        .map(|port| port.storage)
        .or_else(|| module.signal_decl(name).map(|signal| signal.storage))
}

fn read_binding(
    binding: SignalBinding,
    values: &[Value],
    _object_layouts: &[RuntimeObjectLayout],
) -> Result<Value> {
    let value = values.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    Ok(value.coerced_to(binding.view_width))
}

fn write_binding(
    binding: SignalBinding,
    value: Value,
    values: &mut [Value],
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
    let next = value.coerced_to(binding.view_width).coerced_to(object.width);
    let changed = *current != next;
    *current = next;
    Ok(changed)
}

fn seed_runtime_frame(
    module: &ModuleSummary,
    state: &ModuleState,
    persisted: &[Value],
    object_layouts: &[RuntimeObjectLayout],
    inputs: &BTreeMap<String, BitValue>,
) -> Result<Vec<Value>> {
    let mut frame = persisted.to_vec();

    for port in &module.ports {
        if !matches!(port.direction, PortDirection::Input) {
            continue;
        }
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        let value = Value::new(
            inputs
                .get(&port.name)
                .cloned()
                .unwrap_or_else(BitValue::zero),
            port.width(),
        );
        write_binding(binding, value, &mut frame, object_layouts)?;
    }

    for name in inputs.keys() {
        if module.port(name).is_none() {
            return Err(Error::Resolve(format!(
                "input '{}' does not match any port on module '{}'",
                name, module.name
            )));
        }
    }

    Ok(frame)
}

fn build_instance_value_table(
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[Value],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<HashMap<String, Value>> {
    let mut values = HashMap::new();

    for port in &module.ports {
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        values.insert(port.name.clone(), read_binding(binding, frame, object_layouts)?);
    }

    for signal in &module.signals {
        let binding = state.signals.get(&signal.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                signal.name, module.name
            ))
        })?;
        values.insert(signal.name.clone(), read_binding(binding, frame, object_layouts)?);
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

fn sync_instance_values_to_frame(
    module: &ModuleSummary,
    state: &ModuleState,
    values: &HashMap<String, Value>,
    frame: &mut [Value],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<bool> {
    let mut changed = false;

    for port in &module.ports {
        let value = values
            .get(&port.name)
            .cloned()
            .unwrap_or_else(|| Value::zero(port.width()));
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        changed |= write_binding(binding, value, frame, object_layouts)?;
    }

    for signal in &module.signals {
        let value = values
            .get(&signal.name)
            .cloned()
            .unwrap_or_else(|| Value::zero(signal.width()));
        let binding = state.signals.get(&signal.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                signal.name, module.name
            ))
        })?;
        changed |= write_binding(binding, value, frame, object_layouts)?;
    }

    Ok(changed)
}

fn drive_child_inputs(
    parent_module: &ModuleSummary,
    child_state: &ChildState,
    parent_values: &HashMap<String, Value>,
    parent_memories: &HashMap<String, MemoryState>,
    frame: &mut [Value],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<bool> {
    let mut changed = false;

    for driver in &child_state.input_drivers {
        let value = eval_expr(&driver.expr, parent_module, parent_values, parent_memories)?;
        let binding = child_state
            .state
            .signals
            .get(&driver.port_name)
            .copied()
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    driver.port_name, child_state.state.module_name
                ))
            })?;
        changed |= write_binding(binding, value, frame, object_layouts)?;
    }

    Ok(changed)
}

fn snapshot_child_inputs(
    child_module: &ModuleSummary,
    child_state: &ModuleState,
    frame: &[Value],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<BTreeMap<String, BitValue>> {
    let mut inputs = BTreeMap::new();

    for port in child_module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input))
    {
        let binding = child_state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, child_module.name
            ))
        })?;
        inputs.insert(
            port.name.clone(),
            read_binding(binding, frame, object_layouts)?.normalized_bits(),
        );
    }

    Ok(inputs)
}

fn apply_child_output_sinks(
    parent_module: &ModuleSummary,
    parent_state: &ModuleState,
    child_state: &ChildState,
    parent_values: &HashMap<String, Value>,
    parent_memories: &HashMap<String, MemoryState>,
    frame: &mut [Value],
    object_layouts: &[RuntimeObjectLayout],
) -> Result<bool> {
    if child_state.output_sinks.is_empty() {
        return Ok(false);
    }

    let mut next_parent_values = parent_values.clone();
    let mut changed = false;

    for sink in &child_state.output_sinks {
        let binding = child_state
            .state
            .signals
            .get(&sink.port_name)
            .copied()
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    sink.port_name, child_state.state.module_name
                ))
            })?;
        let value = read_binding(binding, frame, object_layouts)?;
        let target = resolve_lvalue(&sink.target, parent_module, &next_parent_values, parent_memories)?;
        let mut no_memories = HashMap::new();
        changed |= apply_resolved_lvalue(
            &target,
            value,
            parent_module,
            &mut next_parent_values,
            &mut no_memories,
        )?;
    }

    changed |= sync_instance_values_to_frame(
        parent_module,
        parent_state,
        &next_parent_values,
        frame,
        object_layouts,
    )?;
    Ok(changed)
}

fn execute_proc_block(
    kind: &ProcBlockKind,
    body: &Stmt,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<bool> {
    match kind {
        ProcBlockKind::AlwaysComb => {
            let mut next_values = values.clone();
            execute_comb_stmt(body, module, &mut next_values, memories)?;
            let changed = *values != next_values;
            *values = next_values;
            Ok(changed)
        }
        ProcBlockKind::AlwaysFf { .. } => Ok(false),
    }
}

fn execute_comb_stmt(
    stmt: &Stmt,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<()> {
    match stmt {
        Stmt::Empty => Ok(()),
        Stmt::Block(statements) => {
            for statement in statements {
                execute_comb_stmt(statement, module, values, memories)?;
            }
            Ok(())
        }
        Stmt::Assign { kind, target, expr } => match kind {
            AssignmentKind::Blocking => {
                let value = eval_expr(expr, module, values, memories)?;
                let target = resolve_lvalue(target, module, values, memories)?;
                if resolved_lvalue_contains_memory(&target) {
                    return Err(Error::Unsupported(
                        "memory element assignments are only supported inside `always_ff` blocks"
                            .into(),
                    ));
                }
                let mut no_memories = HashMap::new();
                apply_resolved_lvalue(&target, value, module, values, &mut no_memories)?;
                Ok(())
            }
            AssignmentKind::Nonblocking => Err(Error::Unsupported(
                "nonblocking assignments are only supported inside `always_ff` blocks".into(),
            )),
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if eval_expr(cond, module, values, memories)?.truthy() {
                execute_comb_stmt(then_branch, module, values, memories)
            } else if let Some(else_branch) = else_branch {
                execute_comb_stmt(else_branch, module, values, memories)
            } else {
                Ok(())
            }
        }
        Stmt::Case {
            expr,
            items,
            default,
        } => {
            let value = eval_expr(expr, module, values, memories)?;
            for item in items {
                for pattern in &item.patterns {
                    if values_equal(&value, &eval_expr(pattern, module, values, memories)?) {
                        return execute_comb_stmt(&item.body, module, values, memories);
                    }
                }
            }
            if let Some(default) = default {
                execute_comb_stmt(default, module, values, memories)
            } else {
                Ok(())
            }
        }
    }
}

fn execute_sequential_stmt(
    stmt: &Stmt,
    module: &ModuleSummary,
    current_values: &mut HashMap<String, Value>,
    memories: &mut HashMap<String, MemoryState>,
    staged_values: &mut HashMap<String, Value>,
    staged_memories: &mut HashMap<String, MemoryState>,
) -> Result<()> {
    match stmt {
        Stmt::Empty => Ok(()),
        Stmt::Block(statements) => {
            for statement in statements {
                execute_sequential_stmt(
                    statement,
                    module,
                    current_values,
                    memories,
                    staged_values,
                    staged_memories,
                )?;
            }
            Ok(())
        }
        Stmt::Assign { kind, target, expr } => match kind {
            AssignmentKind::Nonblocking => {
                let value = eval_expr(expr, module, current_values, memories)?;
                let target = resolve_lvalue(target, module, current_values, memories)?;
                apply_resolved_lvalue(&target, value, module, staged_values, staged_memories)?;
                Ok(())
            }
            AssignmentKind::Blocking => {
                let value = eval_expr(expr, module, current_values, memories)?;
                let target = resolve_lvalue(target, module, current_values, memories)?;
                apply_resolved_lvalue(&target, value.clone(), module, current_values, memories)?;
                apply_resolved_lvalue(&target, value, module, staged_values, staged_memories)?;
                Ok(())
            }
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if eval_expr(cond, module, current_values, memories)?.truthy() {
                execute_sequential_stmt(
                    then_branch,
                    module,
                    current_values,
                    memories,
                    staged_values,
                    staged_memories,
                )
            } else if let Some(else_branch) = else_branch {
                execute_sequential_stmt(
                    else_branch,
                    module,
                    current_values,
                    memories,
                    staged_values,
                    staged_memories,
                )
            } else {
                Ok(())
            }
        }
        Stmt::Case {
            expr,
            items,
            default,
        } => {
            let value = eval_expr(expr, module, current_values, memories)?;
            for item in items {
                for pattern in &item.patterns {
                    if values_equal(
                        &value,
                        &eval_expr(pattern, module, current_values, memories)?,
                    ) {
                        return execute_sequential_stmt(
                            &item.body,
                            module,
                            current_values,
                            memories,
                            staged_values,
                            staged_memories,
                        );
                    }
                }
            }
            if let Some(default) = default {
                execute_sequential_stmt(
                    default,
                    module,
                    current_values,
                    memories,
                    staged_values,
                    staged_memories,
                )
            } else {
                Ok(())
            }
        }
    }
}

fn build_memory_table(module: &ModuleSummary) -> HashMap<String, MemoryState> {
    let mut memories = HashMap::new();

    for memory in &module.memories {
        memories.insert(
            memory.name.clone(),
            MemoryState {
                index_range: memory.index_range,
                words: vec![Value::zero(memory.element_width()); memory.depth()],
            },
        );
    }

    memories
}

fn build_clock_state_table(module: &ModuleSummary) -> HashMap<String, bool> {
    let mut clocks = HashMap::new();

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

fn build_legacy_rom_state(
    hir: &HirDesign,
    module: &ModuleSummary,
) -> Result<Option<LegacyRomState>> {
    if !module.name.starts_with("rom_")
        || !module.signals.is_empty()
        || !module.memories.is_empty()
        || !module.continuous_assignments.is_empty()
        || !module.proc_blocks.is_empty()
        || !module.instantiations.is_empty()
    {
        return Ok(None);
    }

    let addr_port = module
        .ports
        .iter()
        .find(|port| matches!(port.direction, PortDirection::Input))
        .ok_or_else(|| {
            Error::Resolve(format!(
                "legacy ROM primitive '{}' requires an input address port",
                module.name
            ))
        })?;
    let data_port = module
        .ports
        .iter()
        .find(|port| matches!(port.direction, PortDirection::Output))
        .ok_or_else(|| {
            Error::Resolve(format!(
                "legacy ROM primitive '{}' requires an output data port",
                module.name
            ))
        })?;
    let source_path = hir.module_source_path(&module.name).ok_or_else(|| {
        Error::Resolve(format!(
            "could not determine source file for legacy ROM primitive '{}'",
            module.name
        ))
    })?;
    let rom_name = &module.name["rom_".len()..];
    if rom_name.is_empty() {
        return Ok(None);
    }

    let data_path = resolve_legacy_rom_data_path(source_path, rom_name).ok_or_else(|| {
        Error::Resolve(format!(
            "legacy ROM primitive '{}' could not find '{}.txt'",
            module.name, rom_name
        ))
    })?;
    let depth = 1usize
        .checked_shl(addr_port.width() as u32)
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "legacy ROM primitive '{}' address width {} exceeds host limits",
                module.name,
                addr_port.width()
            ))
        })?;
    let writes = parse_memory_file(&data_path, data_port.width(), depth)?;
    let mut words = vec![Value::zero(data_port.width()); depth];
    for (index, word) in writes {
        words[index] = Value::new(word, data_port.width());
    }

    Ok(Some(LegacyRomState {
        addr_port: addr_port.name.clone(),
        data_port: data_port.name.clone(),
        words,
    }))
}

fn apply_legacy_rom_outputs(
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
    legacy_rom: &LegacyRomState,
) -> Result<()> {
    let addr = values
        .get(&legacy_rom.addr_port)
        .cloned()
        .ok_or_else(|| {
            Error::Resolve(format!(
                "legacy ROM primitive '{}' is missing address port '{}'",
                module.name, legacy_rom.addr_port
            ))
        })?
        .normalized_bits()
        .to_usize_checked()
        .ok_or_else(|| {
            Error::Resolve(format!(
                "legacy ROM primitive '{}' address exceeds host limits",
                module.name
            ))
        })?;
    let data = legacy_rom.words.get(addr).cloned().ok_or_else(|| {
        Error::Resolve(format!(
            "legacy ROM primitive '{}' address {} is out of range",
            module.name, addr
        ))
    })?;
    values.insert(legacy_rom.data_port.clone(), data);
    Ok(())
}

fn elaborate_module_parameters(
    module: &ModuleSummary,
    parent_module: Option<&ModuleSummary>,
    parent_parameter_values: Option<&HashMap<String, Value>>,
    instance: Option<&ModuleInstanceSummary>,
) -> Result<HashMap<String, Value>> {
    let empty_memories = HashMap::new();
    let mut values = HashMap::new();

    for param in &module.parameters {
        let value = if let Some(override_expr) = instance.and_then(|instance| {
            instance
                .parameter_overrides
                .iter()
                .find(|override_expr| override_expr.parameter_name == param.name)
        }) {
            let parent_module = parent_module.ok_or_else(|| {
                Error::Resolve(format!(
                    "parameter override for '{}' on '{}' is missing parent module context",
                    param.name, module.name
                ))
            })?;
            let parent_values = parent_parameter_values.ok_or_else(|| {
                Error::Resolve(format!(
                    "parameter override for '{}' on '{}' is missing parent parameter values",
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

    Ok(values)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedLValue {
    Signal(String),
    Concat(Vec<ResolvedLValue>),
    BitSelect {
        signal: String,
        index: usize,
    },
    PartSelect {
        signal: String,
        msb: usize,
        lsb: usize,
    },
    MemoryElement {
        memory: String,
        index: usize,
    },
}

fn collect_outputs(
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[Value],
    object_layouts: &[RuntimeObjectLayout],
) -> BTreeMap<String, BitValue> {
    let mut outputs = BTreeMap::new();

    for port in module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output))
    {
        let value = state
            .signals
            .get(&port.name)
            .copied()
            .and_then(|binding| read_binding(binding, frame, object_layouts).ok())
            .unwrap_or_else(|| Value::zero(port.width()))
            .coerced_to(port.width());
        outputs.insert(port.name.clone(), value.normalized_bits());
    }

    outputs
}

fn resolve_supported_module<'a>(
    hir: &'a HirDesign,
    module_name: &str,
) -> Result<&'a ModuleSummary> {
    let module = hir
        .module(module_name)
        .ok_or_else(|| Error::Resolve(format!("module '{}' was not compiled", module_name)))?;
    if !module.unsupported.is_empty() {
        return Err(Error::Unsupported(format!(
            "module '{}' uses unsupported constructs: {}",
            module_name,
            module
                .unsupported
                .iter()
                .map(|diag| diag.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    Ok(module)
}

fn eval_expr(
    expr: &Expr,
    module: &ModuleSummary,
    values: &HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<Value> {
    match expr {
        Expr::Ident(name) => values
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Resolve(format!("signal '{}' is not declared", name))),
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        Expr::Concat(exprs) => {
            let mut values_out = Vec::with_capacity(exprs.len());
            for expr in exprs {
                values_out.push(eval_expr(expr, module, values, memories)?);
            }
            concat_values(&values_out)
        }
        Expr::Repeat { count, expr } => {
            let value = eval_expr(expr, module, values, memories)?;
            let values_out = vec![value; *count];
            concat_values(&values_out)
        }
        Expr::MemoryRead { memory, index } => {
            let index = eval_expr(index, module, values, memories)?
                .normalized_bits()
                .to_usize_checked()
                .ok_or_else(|| Error::Resolve("memory index exceeds host limits".into()))?;
            let memory_state = memories
                .get(memory)
                .ok_or_else(|| Error::Resolve(format!("memory '{}' is not declared", memory)))?;
            memory_state.read(index, memory)
        }
        Expr::BitSelect { expr, index } => {
            let value = eval_expr(expr, module, values, memories)?;
            if *index >= value.width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for width {}",
                    index, value.width
                )));
            }
            Ok(Value::new(
                BitValue::from(value.normalized_bits().get_bit(*index)),
                1,
            ))
        }
        Expr::PartSelect { expr, msb, lsb } => {
            let value = eval_expr(expr, module, values, memories)?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            if high >= value.width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for width {}",
                    msb, lsb, value.width
                )));
            }
            let width = high - low + 1;
            Ok(Value::new(value.normalized_bits().slice(low, width), width))
        }
        Expr::Unary { op, expr } => {
            let value = eval_expr(expr, module, values, memories)?;
            match op {
                UnaryOp::BitNot => Ok(Value::new_with_signed(
                    value.normalized_bits().bitnot_with_width(value.width),
                    value.width,
                    value.signed,
                )),
                UnaryOp::Negate => Ok(Value::new_with_signed(
                    BitValue::zero().wrapping_sub(&value.normalized_bits(), value.width),
                    value.width,
                    value.signed,
                )),
                UnaryOp::LogicalNot => {
                    let is_zero = value.normalized_bits().is_zero();
                    Ok(Value::new(BitValue::from(u64::from(is_zero)), 1))
                }
                UnaryOp::ReductionOr => {
                    let result = !value.normalized_bits().is_zero();
                    Ok(Value::new(BitValue::from(u64::from(result)), 1))
                }
                UnaryOp::ReductionAnd => {
                    let mask = mask(value.width);
                    let result = value.normalized_bits().bitand(&mask) == mask;
                    Ok(Value::new(BitValue::from(u64::from(result)), 1))
                }
                UnaryOp::ReductionNand => {
                    let mask = mask(value.width);
                    let result = value.normalized_bits().bitand(&mask) != mask;
                    Ok(Value::new(BitValue::from(u64::from(result)), 1))
                }
                UnaryOp::ReductionXor => {
                    let mut count = 0u32;
                    let bits = value.normalized_bits();
                    for i in 0..value.width {
                        if !bits.slice(i, 1).is_zero() {
                            count += 1;
                        }
                    }
                    Ok(Value::new(BitValue::from(u64::from(count % 2 != 0)), 1))
                }
                UnaryOp::Signed => Ok(Value::new_with_signed(
                    value.normalized_bits(),
                    value.width,
                    true,
                )),
                UnaryOp::Unsigned => Ok(Value::new_with_signed(
                    value.normalized_bits(),
                    value.width,
                    false,
                )),
            }
        }
        Expr::Binary { left, op, right } => {
            let mut left = eval_expr(left, module, values, memories)?;
            let mut right = eval_expr(right, module, values, memories)?;
            let common_width = left.width.max(right.width);
            left = left.coerced_to(common_width);
            right = right.coerced_to(common_width);
            let (bits, width) = match op {
                BinaryOp::BitAnd => (
                    left.normalized_bits().bitand(&right.normalized_bits()),
                    common_width,
                ),
                BinaryOp::BitOr => (
                    left.normalized_bits().bitor(&right.normalized_bits()),
                    common_width,
                ),
                BinaryOp::BitXor => (
                    left.normalized_bits().bitxor(&right.normalized_bits()),
                    common_width,
                ),
                BinaryOp::ShiftLeft => (
                    shift_left_bits(
                        &left.normalized_bits(),
                        &right.normalized_bits(),
                        left.width,
                    ),
                    left.width,
                ),
                BinaryOp::ShiftRight => (
                    shift_right_bits(
                        &left.normalized_bits(),
                        &right.normalized_bits(),
                        left.width,
                    ),
                    left.width,
                ),
                BinaryOp::ArithmeticShiftRight => (
                    arithmetic_shift_right_bits(
                        &left.normalized_bits(),
                        &right.normalized_bits(),
                        left.width,
                    ),
                    left.width,
                ),
                BinaryOp::LogicalAnd => (BitValue::from(left.truthy() && right.truthy()), 1),
                BinaryOp::LogicalOr => (BitValue::from(left.truthy() || right.truthy()), 1),
                BinaryOp::Eq => (BitValue::from(values_equal(&left, &right)), 1),
                BinaryOp::NotEq => (BitValue::from(!values_equal(&left, &right)), 1),
                BinaryOp::Lt => (BitValue::from(compare_values(&left, &right).is_lt()), 1),
                BinaryOp::LtEq => (BitValue::from(!compare_values(&left, &right).is_gt()), 1),
                BinaryOp::Gt => (BitValue::from(compare_values(&left, &right).is_gt()), 1),
                BinaryOp::GtEq => (BitValue::from(!compare_values(&left, &right).is_lt()), 1),
                BinaryOp::Add => (
                    left.normalized_bits()
                        .wrapping_add(&right.normalized_bits(), common_width),
                    common_width,
                ),
                BinaryOp::Sub => (
                    left.normalized_bits()
                        .wrapping_sub(&right.normalized_bits(), common_width),
                    common_width,
                ),
                BinaryOp::Mul => (
                    left.normalized_bits()
                        .wrapping_mul(&right.normalized_bits(), common_width),
                    common_width,
                ),
            };
            Ok(Value::new_with_signed(
                bits,
                width,
                matches!(
                    op,
                    BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::ArithmeticShiftRight
                ) && left.signed
                    || matches!(
                        op,
                        BinaryOp::BitAnd
                            | BinaryOp::BitOr
                            | BinaryOp::BitXor
                            | BinaryOp::Add
                            | BinaryOp::Sub
                            | BinaryOp::Mul
                    ) && left.signed
                        && right.signed,
            ))
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            let result_width = expr_width(when_true, module)?.max(expr_width(when_false, module)?);
            if eval_expr(cond, module, values, memories)?.truthy() {
                Ok(eval_expr(when_true, module, values, memories)?.coerced_to(result_width))
            } else {
                Ok(eval_expr(when_false, module, values, memories)?.coerced_to(result_width))
            }
        }
    }
}

fn value_from_literal(literal: &NumericLiteral) -> Value {
    let width = literal
        .width
        .unwrap_or_else(|| minimum_width(&literal.bits));
    Value::new(literal.bits.clone(), width)
}

fn concat_values(parts: &[Value]) -> Result<Value> {
    let mut total_width = 0usize;
    for part in parts {
        total_width = total_width
            .checked_add(part.width)
            .ok_or_else(|| Error::Unsupported("concatenation width exceeds host limits".into()))?;
    }

    let mut bits = BitValue::zero();
    let mut shift = total_width;
    for part in parts {
        shift -= part.width;
        bits = bits.bitor(&part.normalized_bits().shift_left(shift));
    }
    Ok(Value::new(bits, total_width))
}

fn values_equal(left: &Value, right: &Value) -> bool {
    let width = left.width.max(right.width);
    left.coerced_to(width).normalized_bits() == right.coerced_to(width).normalized_bits()
}

fn compare_values(left: &Value, right: &Value) -> std::cmp::Ordering {
    if left.signed && right.signed {
        compare_signed_bits(
            &left.normalized_bits(),
            &right.normalized_bits(),
            left.width,
        )
    } else {
        left.normalized_bits()
            .cmp_unsigned(&right.normalized_bits())
    }
}

fn compare_signed_bits(left: &BitValue, right: &BitValue, width: usize) -> std::cmp::Ordering {
    let width = width.max(1);
    let left = left.truncate(width);
    let right = right.truncate(width);
    match left.get_bit(width - 1).cmp(&right.get_bit(width - 1)) {
        std::cmp::Ordering::Less => std::cmp::Ordering::Greater,
        std::cmp::Ordering::Greater => std::cmp::Ordering::Less,
        std::cmp::Ordering::Equal => left.cmp_unsigned(&right),
    }
}

fn resolve_lvalue(
    lvalue: &LValue,
    module: &ModuleSummary,
    values: &HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<ResolvedLValue> {
    match lvalue {
        LValue::Signal(name) => {
            if module.signal_width(name).is_none() {
                return Err(Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                )));
            }
            Ok(ResolvedLValue::Signal(name.clone()))
        }
        LValue::Concat(items) => {
            let mut resolved = Vec::with_capacity(items.len());
            for item in items {
                resolved.push(resolve_lvalue(item, module, values, memories)?);
            }
            Ok(ResolvedLValue::Concat(resolved))
        }
        LValue::BitSelect { signal, index } => Ok(ResolvedLValue::BitSelect {
            signal: signal.clone(),
            index: *index,
        }),
        LValue::PartSelect { signal, msb, lsb } => Ok(ResolvedLValue::PartSelect {
            signal: signal.clone(),
            msb: *msb,
            lsb: *lsb,
        }),
        LValue::MemoryElement { memory, index } => Ok(ResolvedLValue::MemoryElement {
            memory: memory.clone(),
            index: eval_expr(index, module, values, memories)?
                .normalized_bits()
                .to_usize_checked()
                .ok_or_else(|| Error::Resolve("memory index exceeds host limits".into()))?,
        }),
    }
}

fn resolved_lvalue_contains_memory(lvalue: &ResolvedLValue) -> bool {
    match lvalue {
        ResolvedLValue::Signal(_)
        | ResolvedLValue::BitSelect { .. }
        | ResolvedLValue::PartSelect { .. } => false,
        ResolvedLValue::Concat(items) => items.iter().any(resolved_lvalue_contains_memory),
        ResolvedLValue::MemoryElement { .. } => true,
    }
}

fn resolved_lvalue_width(lvalue: &ResolvedLValue, module: &ModuleSummary) -> Result<usize> {
    match lvalue {
        ResolvedLValue::Signal(name) => module.signal_width(name).ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                name, module.name
            ))
        }),
        ResolvedLValue::Concat(items) => {
            let mut total = 0usize;
            for item in items {
                total += resolved_lvalue_width(item, module)?;
            }
            Ok(total)
        }
        ResolvedLValue::BitSelect { signal, index } => {
            let width = module.signal_width(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            if *index >= width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for signal '{}'",
                    index, signal
                )));
            }
            Ok(1)
        }
        ResolvedLValue::PartSelect { signal, msb, lsb } => {
            let width = module.signal_width(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            let high = (*msb).max(*lsb);
            if high >= width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for signal '{}'",
                    msb, lsb, signal
                )));
            }
            Ok(high - (*msb).min(*lsb) + 1)
        }
        ResolvedLValue::MemoryElement { memory, .. } => module
            .memory_decl(memory)
            .map(|memory| memory.element_width())
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            }),
    }
}

fn apply_resolved_lvalue(
    lvalue: &ResolvedLValue,
    value: Value,
    module: &ModuleSummary,
    values: &mut HashMap<String, Value>,
    memories: &mut HashMap<String, MemoryState>,
) -> Result<bool> {
    match lvalue {
        ResolvedLValue::Signal(name) => {
            let current = values.get_mut(name).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                ))
            })?;
            let next = value.coerced_to(current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::Concat(items) => {
            let total_width = resolved_lvalue_width(lvalue, module)?;
            let normalized = value.coerced_to(total_width).normalized_bits();
            let mut remaining_width = total_width;
            let mut changed = false;
            for item in items {
                let item_width = resolved_lvalue_width(item, module)?;
                remaining_width -= item_width;
                let chunk = Value::new(normalized.slice(remaining_width, item_width), item_width);
                changed |= apply_resolved_lvalue(item, chunk, module, values, memories)?;
            }
            Ok(changed)
        }
        ResolvedLValue::BitSelect { signal, index } => {
            let current = values.get_mut(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            if *index >= current.width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for signal '{}'",
                    index, signal
                )));
            }
            let bit = value.coerced_to(1).normalized_bits().get_bit(0);
            let mut bits = current.normalized_bits();
            bits.set_bit(*index, bit);
            let next = Value::new(bits, current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::PartSelect { signal, msb, lsb } => {
            let current = values.get_mut(signal).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            if high >= current.width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for signal '{}'",
                    msb, lsb, signal
                )));
            }
            let width = high - low + 1;
            let mut bits = current.normalized_bits();
            let cleared =
                bits.bitand(&mask(width).shift_left(low).bitnot_with_width(current.width));
            bits = cleared.bitor(&value.coerced_to(width).normalized_bits().shift_left(low));
            let next = Value::new(bits, current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::MemoryElement { memory, index } => {
            let memory_state = memories.get_mut(memory).ok_or_else(|| {
                Error::Resolve(format!(
                    "memory '{}' is not declared in '{}'",
                    memory, module.name
                ))
            })?;
            memory_state.write(*index, value, memory)
        }
    }
}

fn resolve_instance_path<'a>(
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

fn resolve_instance_path_mut<'a>(
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::SimulationSession;
    use crate::{BitValue, Compiler};

    fn bv(value: u64) -> BitValue {
        BitValue::from(value)
    }

    fn inputs<const N: usize>(pairs: [(String, u64); N]) -> BTreeMap<String, BitValue> {
        pairs
            .into_iter()
            .map(|(name, value)| (name, bv(value)))
            .collect()
    }

    fn words<const N: usize>(values: [u64; N]) -> Vec<BitValue> {
        values.into_iter().map(bv).collect()
    }

    fn step_posedge<const N: usize>(
        sim: &mut SimulationSession,
        pairs: [(String, u64); N],
    ) -> BTreeMap<String, BitValue> {
        let mut low_inputs = inputs(pairs.clone());
        low_inputs.insert("clk".into(), bv(0));
        sim.step(low_inputs).expect("step low");

        let mut high_inputs = inputs(pairs);
        high_inputs.insert("clk".into(), bv(1));
        sim.step(high_inputs).expect("step high")
    }

    fn persisted_u64(sim: &super::SimulationSession, state: &super::ModuleState, name: &str) -> u64 {
        let binding = state
            .signals
            .get(name)
            .copied()
            .unwrap_or_else(|| panic!("missing signal binding '{name}'"));
        super::read_binding(binding, &sim.persisted, &sim.objects)
            .expect("read persisted binding")
            .normalized_bits()
            .to_u64_checked()
            .expect("persisted value fits in u64")
    }

    fn memory_u64(state: &super::ModuleState, name: &str, index: usize) -> u64 {
        state
            .memories
            .get(name)
            .unwrap_or_else(|| panic!("missing memory '{name}'"))
            .read(index, name)
            .expect("read memory")
            .normalized_bits()
            .to_u64_checked()
            .expect("memory value fits in u64")
    }

    fn child_state<'a>(state: &'a super::ModuleState, name: &str) -> &'a super::ChildState {
        state
            .children
            .iter()
            .find(|child| child.instance_name == name)
            .unwrap_or_else(|| panic!("missing child instance '{name}'"))
    }

    macro_rules! assert_signal_eq {
        ($outputs:expr, $name:expr, $value:expr) => {
            assert_eq!($outputs.get($name).cloned(), Some(bv($value)));
        };
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn eval_once_runs_leaf_assign_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/nand_gate.sv"))
            .expect("compile nand gate");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([("inA".into(), 1), ("inB".into(), 1)]))
            .expect("eval");

        assert_signal_eq!(outputs, "outY", 0);
    }

    #[test]
    fn eval_once_runs_hierarchical_combinational_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/full_adder.sv"))
            .expect("compile full adder");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("inA".into(), 1),
                ("inB".into(), 1),
                ("inCarry".into(), 1),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "outSum", 1);
        assert_signal_eq!(outputs, "outCarry", 1);
    }

    #[test]
    fn structural_runtime_shares_sibling_net_bindings() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module producer(output wire y); ",
                    "assign y = 1'b1; ",
                    "endmodule\n",
                    "module consumer(input wire a, output logic y); ",
                    "assign y = a; ",
                    "endmodule\n",
                    "module top(output logic out); ",
                    "wire link; ",
                    "producer u_prod(.y(link)); ",
                    "consumer u_cons(.a(link), .y(out)); ",
                    "endmodule\n"
                ),
            )
            .expect("compile structural fixture");
        let mut sim = design.instantiate_top().expect("instantiate");

        let top_link = sim
            .state
            .signals
            .get("link")
            .copied()
            .expect("top link binding");
        let producer_y = child_state(&sim.state, "u_prod")
            .state
            .signals
            .get("y")
            .copied()
            .expect("producer y binding");
        let consumer_a = child_state(&sim.state, "u_cons")
            .state
            .signals
            .get("a")
            .copied()
            .expect("consumer a binding");

        assert_eq!(top_link.object_id, producer_y.object_id);
        assert_eq!(top_link.object_id, consumer_a.object_id);

        let outputs = sim.eval_once(BTreeMap::new()).expect("eval");
        assert_signal_eq!(outputs, "out", 1);
    }

    #[test]
    fn structural_runtime_aliases_input_bindings_across_width_changes() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module pass4(input wire [3:0] a, output logic [3:0] y); ",
                    "assign y = a; ",
                    "endmodule\n",
                    "module top(input logic in, output logic [3:0] out); ",
                    "pass4 u_pass(.a(in), .y(out)); ",
                    "endmodule\n"
                ),
            )
            .expect("compile width alias fixture");
        let mut sim = design.instantiate_top().expect("instantiate");

        let top_in = sim
            .state
            .signals
            .get("in")
            .copied()
            .expect("top input binding");
        let child_a = child_state(&sim.state, "u_pass")
            .state
            .signals
            .get("a")
            .copied()
            .expect("child input binding");

        assert_eq!(top_in.object_id, child_a.object_id);
        assert_eq!(top_in.view_width, 1);
        assert_eq!(child_a.view_width, 4);

        let outputs = sim
            .eval_once(inputs([("in".into(), 1)]))
            .expect("eval width alias");
        assert_signal_eq!(outputs, "out", 0b0001);
    }

    #[test]
    fn eval_once_runs_vector_ternary_assign() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/ternary_mux.sv"))
            .expect("compile ternary mux");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("a".into(), 0x12),
                ("b".into(), 0x34),
                ("sel".into(), 1),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 0x12);
    }

    #[test]
    fn eval_once_normalizes_ternary_width_before_concatenation() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic sel, ",
                    "output logic [3:0] out",
                    "); ",
                    "assign out = {2'b10, (sel ? 2'b11 : 1'b1)}; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("sel".into(), 0)]))
            .expect("eval false branch");
        assert_signal_eq!(outputs, "out", 0b1001);

        let outputs = sim
            .eval_once(inputs([("sel".into(), 1)]))
            .expect("eval true branch");
        assert_signal_eq!(outputs, "out", 0b1011);
    }

    #[test]
    fn eval_once_normalizes_ternary_width_before_replication() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic sel, ",
                    "output logic [3:0] out",
                    "); ",
                    "assign out = {2{sel ? 2'b10 : 1'b1}}; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("sel".into(), 0)]))
            .expect("eval false branch");
        assert_signal_eq!(outputs, "out", 0b0101);

        let outputs = sim
            .eval_once(inputs([("sel".into(), 1)]))
            .expect("eval true branch");
        assert_signal_eq!(outputs, "out", 0b1010);
    }

    #[test]
    fn eval_once_keeps_nested_ternary_false_branches_after_param_folding() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic [31:0] in, ",
                    "output logic [31:0] out",
                    "); ",
                    "localparam logic A = 1'b0; ",
                    "localparam logic B = 1'b0; ",
                    "assign out = A ? 32'h11111111 : B ? 32'h22222222 : in; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("in".into(), 0x0010_0093)]))
            .expect("eval nested ternary");
        assert_signal_eq!(outputs, "out", 0x0010_0093);
    }

    #[test]
    fn eval_once_gives_conditional_lower_precedence_than_logical_and() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic gate, ",
                    "input logic [31:0] in, ",
                    "output logic [31:0] out",
                    "); ",
                    "localparam logic A = 1'b0; ",
                    "assign out = A && gate ? 32'h11111111 : in; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("gate".into(), 1), ("in".into(), 0x0010_0093)]))
            .expect("eval conditional precedence");
        assert_signal_eq!(outputs, "out", 0x0010_0093);
    }

    #[test]
    fn eval_once_gives_equality_higher_precedence_than_logical_and() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic [31:0] in, ",
                    "output logic is_shift_imm",
                    "); ",
                    "assign is_shift_imm = |{",
                    "in[14:12] == 3'b001 && in[31:25] == 7'b0000000, ",
                    "in[14:12] == 3'b101 && in[31:25] == 7'b0000000, ",
                    "in[14:12] == 3'b101 && in[31:25] == 7'b0100000",
                    "}; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("in".into(), 0x0010_0093)]))
            .expect("eval addi helper");
        assert_signal_eq!(outputs, "is_shift_imm", 0);

        let outputs = sim
            .eval_once(inputs([("in".into(), 0x0010_1093)]))
            .expect("eval slli helper");
        assert_signal_eq!(outputs, "is_shift_imm", 1);
    }

    #[test]
    fn eval_once_coerces_assignment_and_instance_port_widths() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module pass4(",
                    "input logic [3:0] in, ",
                    "output logic [3:0] out",
                    "); ",
                    "assign out = in; ",
                    "endmodule\n",
                    "module pass2(",
                    "input logic [1:0] in, ",
                    "output logic [1:0] out",
                    "); ",
                    "assign out = in; ",
                    "endmodule\n",
                    "module bit_driver(",
                    "output logic out",
                    "); ",
                    "assign out = 1'b1; ",
                    "endmodule\n",
                    "module bus_driver(",
                    "output logic [4:0] out",
                    "); ",
                    "assign out = 5'b10101; ",
                    "endmodule\n",
                    "module top(",
                    "input logic a, ",
                    "input logic [7:0] wide_in, ",
                    "output logic [3:0] assign_widened, ",
                    "output logic [1:0] assign_narrowed, ",
                    "output logic [3:0] child_input_widened, ",
                    "output logic [1:0] child_input_narrowed, ",
                    "output logic [5:0] child_output_widened, ",
                    "output logic [2:0] child_output_narrowed",
                    "); ",
                    "assign assign_widened = a; ",
                    "assign assign_narrowed = wide_in; ",
                    "pass4 widen_input(.in(a), .out(child_input_widened)); ",
                    "pass2 narrow_input(.in(wide_in), .out(child_input_narrowed)); ",
                    "bit_driver widen_output(.out(child_output_widened)); ",
                    "bus_driver narrow_output(.out(child_output_narrowed)); ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("a".into(), 1), ("wide_in".into(), 0xab)]))
            .expect("eval coercion case");
        assert_signal_eq!(outputs, "assign_widened", 0b0001);
        assert_signal_eq!(outputs, "assign_narrowed", 0b11);
        assert_signal_eq!(outputs, "child_input_widened", 0b0001);
        assert_signal_eq!(outputs, "child_input_narrowed", 0b11);
        assert_signal_eq!(outputs, "child_output_widened", 0b000001);
        assert_signal_eq!(outputs, "child_output_narrowed", 0b101);

        let outputs = sim
            .eval_once(inputs([("a".into(), 0), ("wide_in".into(), 0x04)]))
            .expect("eval truncated-away bits case");
        assert_signal_eq!(outputs, "assign_widened", 0);
        assert_signal_eq!(outputs, "assign_narrowed", 0);
        assert_signal_eq!(outputs, "child_input_widened", 0);
        assert_signal_eq!(outputs, "child_input_narrowed", 0);
        assert_signal_eq!(outputs, "child_output_widened", 0b000001);
        assert_signal_eq!(outputs, "child_output_narrowed", 0b101);
    }

    #[test]
    fn eval_once_runs_shift_operators_with_left_operand_width() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic [7:0] in, ",
                    "input logic [3:0] shamt, ",
                    "output logic [7:0] left_shifted, ",
                    "output logic [7:0] right_shifted, ",
                    "output logic [7:0] right_past_width",
                    "); ",
                    "assign left_shifted = in << shamt; ",
                    "assign right_shifted = in >> shamt; ",
                    "assign right_past_width = in >> 4'd8; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("in".into(), 0x81), ("shamt".into(), 2)]))
            .expect("eval truncating shift case");
        assert_signal_eq!(outputs, "left_shifted", 0x04);
        assert_signal_eq!(outputs, "right_shifted", 0x20);
        assert_signal_eq!(outputs, "right_past_width", 0x00);

        let outputs = sim
            .eval_once(inputs([("in".into(), 0x03), ("shamt".into(), 6)]))
            .expect("eval large variable shift case");
        assert_signal_eq!(outputs, "left_shifted", 0xc0);
        assert_signal_eq!(outputs, "right_shifted", 0x00);
        assert_signal_eq!(outputs, "right_past_width", 0x00);
    }

    #[test]
    fn eval_once_treats_unsized_decimal_literals_as_32_bit_values() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic [31:0] in, ",
                    "output logic [31:0] out",
                    "); ",
                    "assign out = in & ~1; ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("in".into(), 21)]))
            .expect("eval masked value");
        assert_signal_eq!(outputs, "out", 20);
    }

    #[test]
    fn eval_once_runs_part_select_rewrites() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/013-Vector2.sv"))
            .expect("compile vector test");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([("in".into(), 0x1122_3344)]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 0x4433_2211);
    }

    #[test]
    fn eval_once_runs_always_comb_case_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/mux_4to1_comb.sv"))
            .expect("compile mux_4to1_comb");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("d0".into(), 10),
                ("d1".into(), 20),
                ("d2".into(), 30),
                ("d3".into(), 40),
                ("sel".into(), 2),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 30);
    }

    #[test]
    fn eval_once_runs_always_comb_if_else_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/alu_1bit.sv"))
            .expect("compile alu_1bit");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("a".into(), 0b1010_1010),
                ("b".into(), 0b1100_1100),
                ("op".into(), 0b01),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 0b1110_1110);
    }

    #[test]
    fn eval_once_runs_always_comb_case_with_arithmetic() {
        let temp_dir = unique_temp_dir("always-comb-arithmetic");
        let source = r#"
module arithmetic_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic       sel,
    output logic [7:0] out
);
    always_comb
        if (sel == 1'b0)
            out = a + b;
        else
            out = a - b;
endmodule
"#;
        fs::write(temp_dir.join("arithmetic_ops.sv"), source).expect("write arithmetic_ops");

        let design = Compiler::new()
            .compile_file(temp_dir.join("arithmetic_ops.sv"))
            .expect("compile arithmetic_ops");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("a".into(), 5),
                ("b".into(), 3),
                ("sel".into(), 0),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 8);
    }

    #[test]
    fn eval_once_runs_always_comb_with_logical_operators() {
        let temp_dir = unique_temp_dir("always-comb-logical");
        let source = r#"
module logical_ops (
    input  logic a,
    input  logic b,
    output logic out
);
    always_comb
        if ((a == 1'b0) && (b != 1'b0))
            out = 1'b1;
        else
            out = 1'b0;
endmodule
"#;
        fs::write(temp_dir.join("logical_ops.sv"), source).expect("write logical_ops");

        let design = Compiler::new()
            .compile_file(temp_dir.join("logical_ops.sv"))
            .expect("compile logical_ops");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([("a".into(), 0), ("b".into(), 1)]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 1);
    }

    #[test]
    fn eval_once_runs_always_comb_with_relational_operators() {
        let temp_dir = unique_temp_dir("always-comb-relational");
        let source = r#"
module relational_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    output logic lt,
    output logic le,
    output logic gt,
    output logic ge
);
    assign lt = a < b;
    assign le = a <= b;
    assign gt = a > b;
    assign ge = a >= b;
endmodule
"#;
        fs::write(temp_dir.join("relational_ops.sv"), source).expect("write relational_ops");

        let design = Compiler::new()
            .compile_file(temp_dir.join("relational_ops.sv"))
            .expect("compile relational_ops");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("a".into(), 3), ("b".into(), 5)]))
            .expect("eval lt");
        assert_signal_eq!(outputs, "lt", 1);
        assert_signal_eq!(outputs, "le", 1);
        assert_signal_eq!(outputs, "gt", 0);
        assert_signal_eq!(outputs, "ge", 0);

        let outputs = sim
            .eval_once(inputs([("a".into(), 5), ("b".into(), 5)]))
            .expect("eval eq");
        assert_signal_eq!(outputs, "lt", 0);
        assert_signal_eq!(outputs, "le", 1);
        assert_signal_eq!(outputs, "gt", 0);
        assert_signal_eq!(outputs, "ge", 1);
    }

    #[test]
    fn eval_once_supports_signed_cast_compare_and_shift() {
        let temp_dir = unique_temp_dir("signed-cast-ops");
        let source = r#"
module signed_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    input  logic [2:0] sh,
    output logic       lt,
    output logic [7:0] sra
);
    assign lt = $signed(a) < $signed(b);
    assign sra = $signed(a) >>> sh;
endmodule
"#;
        fs::write(temp_dir.join("signed_ops.sv"), source).expect("write signed_ops");

        let design = Compiler::new()
            .compile_file(temp_dir.join("signed_ops.sv"))
            .expect("compile signed_ops");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([
                ("a".into(), 0xf0),
                ("b".into(), 0x01),
                ("sh".into(), 2),
            ]))
            .expect("eval signed ops");

        assert_signal_eq!(outputs, "lt", 1);
        assert_signal_eq!(outputs, "sra", 0xfc);
    }

    #[test]
    fn eval_once_supports_unsigned_cast_compare_and_unary_negation() {
        let temp_dir = unique_temp_dir("unsigned-cast-negate");
        let source = r#"
module unsigned_negate_ops (
    input  logic [7:0] a,
    input  logic [7:0] b,
    output logic       lt_signed,
    output logic       lt_unsigned,
    output logic [7:0] neg
);
    assign lt_signed = $signed(a) < $signed(b);
    assign lt_unsigned = $unsigned($signed(a)) < $unsigned($signed(b));
    assign neg = -a;
endmodule
"#;
        fs::write(temp_dir.join("unsigned_negate_ops.sv"), source)
            .expect("write unsigned_negate_ops");

        let design = Compiler::new()
            .compile_file(temp_dir.join("unsigned_negate_ops.sv"))
            .expect("compile unsigned_negate_ops");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("a".into(), 0xf0), ("b".into(), 0x01)]))
            .expect("eval unsigned negate ops");

        assert_signal_eq!(outputs, "lt_signed", 1);
        assert_signal_eq!(outputs, "lt_unsigned", 0);
        assert_signal_eq!(outputs, "neg", 0x10);
    }

    #[test]
    fn eval_once_runs_always_comb_with_multiple_assignments_to_same_output() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_alu_8bit.sv"))
            .expect("compile overture_alu_8bit");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("inA".into(), 5),
                ("inB".into(), 3),
                ("op".into(), 0b100),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "outY", 8);
    }

    #[test]
    fn step_runs_register_8bit_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/register_8bit.sv"))
            .expect("compile register_8bit");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = step_posedge(&mut sim, [("enable".into(), 1), ("data".into(), 0x5a)]);
        assert_signal_eq!(outputs, "q", 0x5a);

        let outputs = step_posedge(&mut sim, [("enable".into(), 0), ("data".into(), 0xff)]);
        assert_signal_eq!(outputs, "q", 0x5a);
    }

    #[test]
    fn step_runs_counter_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/counter8.sv"))
            .expect("compile counter8");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = step_posedge(&mut sim, [("reset".into(), 1), ("enable".into(), 0)]);
        assert_signal_eq!(outputs, "count", 0);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("enable".into(), 1)]);
        assert_signal_eq!(outputs, "count", 1);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("enable".into(), 0)]);
        assert_signal_eq!(outputs, "count", 1);
    }

    #[test]
    fn step_persists_blocking_assignments_in_clocked_blocks() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic clk, ",
                    "input logic reset, ",
                    "output logic [3:0] q",
                    "); ",
                    "always @(posedge clk) begin ",
                    "if (reset) q = 4'd0; else q = q + 4'd1; ",
                    "end ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = step_posedge(&mut sim, [("reset".into(), 1)]);
        assert_signal_eq!(outputs, "q", 0);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0)]);
        assert_signal_eq!(outputs, "q", 1);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0)]);
        assert_signal_eq!(outputs, "q", 2);
    }

    #[test]
    fn step_runs_hierarchical_regfile() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/regfile_8x8.sv"))
            .expect("compile regfile_8x8");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = step_posedge(
            &mut sim,
            [
                ("write_en".into(), 1),
                ("write_addr".into(), 3),
                ("write_data".into(), 0x42),
                ("read_addr1".into(), 3),
                ("read_addr2".into(), 0),
            ],
        );
        assert_signal_eq!(outputs, "read_data1", 0x42);

        let outputs = step_posedge(
            &mut sim,
            [
                ("write_en".into(), 1),
                ("write_addr".into(), 1),
                ("write_data".into(), 0x99),
                ("read_addr1".into(), 1),
                ("read_addr2".into(), 3),
            ],
        );
        assert_signal_eq!(outputs, "read_data1", 0x99);
        assert_signal_eq!(outputs, "read_data2", 0x42);
    }

    #[test]
    fn step_runs_overture_pc_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_pc_8bit.sv"))
            .expect("compile overture_pc_8bit");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = step_posedge(
            &mut sim,
            [
                ("reset".into(), 1),
                ("run".into(), 0),
                ("jump_en".into(), 0),
                ("jump_addr".into(), 0),
            ],
        );
        assert_signal_eq!(outputs, "pc", 0);

        let outputs = step_posedge(
            &mut sim,
            [
                ("reset".into(), 0),
                ("run".into(), 1),
                ("jump_en".into(), 0),
                ("jump_addr".into(), 0),
            ],
        );
        assert_signal_eq!(outputs, "pc", 1);

        let outputs = step_posedge(
            &mut sim,
            [
                ("reset".into(), 0),
                ("run".into(), 1),
                ("jump_en".into(), 1),
                ("jump_addr".into(), 10),
            ],
        );
        assert_signal_eq!(outputs, "pc", 10);
    }

    #[test]
    fn eval_once_reads_zero_initialized_memory() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_fetch.sv"))
            .expect("compile overture_fetch");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .eval_once(inputs([("addr".into(), 0x2a)]))
            .expect("eval");

        assert_signal_eq!(outputs, "data", 0);
    }

    #[test]
    fn eval_once_reads_preloaded_memory() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_fetch.sv"))
            .expect("compile overture_fetch");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_words(&[], "rom", &words([0x12, 0x34, 0x56]))
            .expect("load rom");

        let outputs = sim.eval_once(inputs([("addr".into(), 1)])).expect("eval");

        assert_signal_eq!(outputs, "data", 0x34);
    }

    #[test]
    fn eval_once_reads_memory_loaded_from_binary_text_file() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_fetch.sv"))
            .expect("compile overture_fetch");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_file(&[], "rom", repo.join("parts/basic/deadbeef.txt"))
            .expect("load rom from file");

        let outputs = sim.eval_once(inputs([("addr".into(), 2)])).expect("eval");

        assert_signal_eq!(outputs, "data", 0xbe);
    }

    #[test]
    fn load_memory_file_supports_sparse_address_overrides() {
        let temp_dir = unique_temp_dir("memory-file-addresses");
        let memory_file = temp_dir.join("sparse_rom.txt");
        fs::write(
            &memory_file,
            "\
// leave address 0 untouched
2: 0x2a
3: 0b0000_1111
",
        )
        .expect("write sparse memory file");

        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/overture/overture_fetch.sv"))
            .expect("compile overture_fetch");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_file(&[], "rom", &memory_file)
            .expect("load rom from sparse file");

        let outputs = sim
            .eval_once(inputs([("addr".into(), 0)]))
            .expect("eval addr 0");
        assert_signal_eq!(outputs, "data", 0);

        let outputs = sim
            .eval_once(inputs([("addr".into(), 2)]))
            .expect("eval addr 2");
        assert_signal_eq!(outputs, "data", 0x2a);

        let outputs = sim
            .eval_once(inputs([("addr".into(), 3)]))
            .expect("eval addr 3");
        assert_signal_eq!(outputs, "data", 0x0f);
    }

    #[test]
    fn step_runs_memory_cpu_stub_with_preloaded_rom_and_ram_write() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/memory_cpu_stub.sv"))
            .expect("compile memory_cpu_stub");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_words(&[], "rom", &words([0x03, 0x42, 0x80, 0xc0]))
            .expect("load rom");

        let outputs = step_posedge(&mut sim, [("reset".into(), 1), ("run".into(), 0)]);
        assert_signal_eq!(outputs, "pc", 0);
        assert_signal_eq!(outputs, "acc", 0);
        assert_signal_eq!(outputs, "ram_out", 0);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("run".into(), 1)]);
        assert_signal_eq!(outputs, "pc", 1);
        assert_signal_eq!(outputs, "acc", 3);
        assert_signal_eq!(outputs, "ram_out", 0);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("run".into(), 1)]);
        assert_signal_eq!(outputs, "pc", 2);
        assert_signal_eq!(outputs, "acc", 5);
        assert_signal_eq!(outputs, "ram_out", 0);

        let outputs = step_posedge(&mut sim, [("reset".into(), 0), ("run".into(), 1)]);
        assert_signal_eq!(outputs, "pc", 3);
        assert_signal_eq!(outputs, "acc", 5);
        assert_signal_eq!(outputs, "ram_out", 5);
        assert_eq!(
            sim.read_memory_word(&[], "ram", 0).expect("read ram"),
            bv(5)
        );
    }

    #[test]
    fn step_runs_overture_cpu_with_preloaded_child_rom() {
        let repo = repo_root();
        let design = Compiler::new()
            .add_search_path(repo.join("parts/overture"))
            .compile_file(repo.join("parts/overture/overture_cpu.sv"))
            .expect("compile overture_cpu");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_words(&["fetch_unit"], "rom", &words([0x05]))
            .expect("load child rom");

        let outputs = step_posedge(
            &mut sim,
            [
                ("reset".into(), 1),
                ("run".into(), 0),
                ("in_port".into(), 0),
            ],
        );
        assert_signal_eq!(outputs, "pc", 0);

        let outputs = step_posedge(
            &mut sim,
            [
                ("reset".into(), 0),
                ("run".into(), 1),
                ("in_port".into(), 0),
            ],
        );
        assert_signal_eq!(outputs, "pc", 1);
        assert_signal_eq!(outputs, "instr_debug", 0x05);
        assert_signal_eq!(outputs, "r0_out", 0x05);
        assert_eq!(
            sim.read_memory_word(&["fetch_unit"], "rom", 0)
                .expect("read child rom"),
            bv(0x05)
        );
    }

    #[test]
    fn instantiate_top_supports_picorv32_parameterized_wrapper() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/picorv32/picorv32.v"))
            .expect("compile picorv32");

        let _sim = design.instantiate_top().expect("instantiate picorv32_wb");
    }

    #[test]
    fn eval_once_applies_named_parameter_overrides_from_parent_modules() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module leaf #(parameter [7:0] VALUE = 8'h11)(",
                    "output logic [7:0] out",
                    "); ",
                    "assign out = VALUE; ",
                    "endmodule\n",
                    "module top #(parameter [7:0] VALUE = 8'h2a)(",
                    "output logic [7:0] out",
                    "); ",
                    "leaf #(.VALUE(VALUE)) u_leaf(.out(out)); ",
                    "endmodule\n"
                ),
            )
            .expect("compile virtual design");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim.eval_once(BTreeMap::new()).expect("eval");

        assert_signal_eq!(outputs, "out", 0x2a);
    }

    #[test]
    fn step_runs_picorv32_smoke_store_sequence() {
        let repo = repo_root();
        let design = Compiler::new()
            .add_search_path(repo.join("parts/picorv32"))
            .compile_file(repo.join("parts/picorv32/picorv32_smoke.sv"))
            .expect("compile picorv32 smoke harness");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_file(
            &[],
            "rom",
            repo.join("parts/picorv32/picorv32_smoke_rom.txt"),
        )
        .expect("load smoke rom");

        step_posedge(&mut sim, [("resetn".into(), 0)]);

        for _ in 0..9 {
            step_posedge(&mut sim, [("resetn".into(), 1)]);
        }

        let core = sim
            .state
            .children
            .first()
            .expect("child instance")
            .state
            .as_ref();
        assert_eq!(memory_u64(core, "cpuregs", 1), 1);
        assert_eq!(persisted_u64(&sim, core, "mem_do_wdata"), 1);
        assert_eq!(persisted_u64(&sim, core, "reg_op1"), 8);
        assert_eq!(persisted_u64(&sim, core, "reg_op2"), 1);

        let outputs = step_posedge(&mut sim, [("resetn".into(), 1)]);
        assert_signal_eq!(outputs, "trap", 0);
        assert_signal_eq!(outputs, "mem_valid", 1);
        assert_signal_eq!(outputs, "mem_instr", 0);
        assert_signal_eq!(outputs, "mem_addr", 8);
        assert_signal_eq!(outputs, "store_seen", 0);

        let outputs = step_posedge(&mut sim, [("resetn".into(), 1)]);
        assert_signal_eq!(outputs, "store_seen", 1);
        assert_signal_eq!(outputs, "store_addr", 8);
        assert_signal_eq!(outputs, "store_data", 1);
    }

    #[test]
    fn step_runs_always_ff_only_on_rising_edges() {
        let temp_dir = unique_temp_dir("always-ff-rising-edge");
        let design = Compiler::new()
            .compile_str(
                temp_dir.join("edge_counter.sv"),
                r#"
module edge_counter(
    input  logic clk,
    output logic [7:0] count
);
    always_ff @(posedge clk)
        count <= count + 1'b1;
endmodule
"#,
            )
            .expect("compile edge_counter");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim.step(inputs([("clk".into(), 0)])).expect("step low");
        assert_signal_eq!(outputs, "count", 0);

        let outputs = sim.step(inputs([("clk".into(), 1)])).expect("step rise");
        assert_signal_eq!(outputs, "count", 1);

        let outputs = sim.step(inputs([("clk".into(), 1)])).expect("step high");
        assert_signal_eq!(outputs, "count", 1);

        let outputs = sim.step(inputs([("clk".into(), 0)])).expect("step fall");
        assert_signal_eq!(outputs, "count", 1);

        let outputs = sim
            .step(inputs([("clk".into(), 1)]))
            .expect("step rise again");
        assert_signal_eq!(outputs, "count", 2);
    }

    #[test]
    fn step_runs_always_ff_on_async_reset_edges() {
        let temp_dir = unique_temp_dir("always-ff-async-reset");
        let design = Compiler::new()
            .compile_str(
                temp_dir.join("async_reset_counter.sv"),
                r#"
module async_reset_counter(
    input  logic clk,
    input  logic reset,
    output logic [7:0] count
);
    always_ff @(posedge clk or posedge reset)
        if (reset)
            count <= 8'd0;
        else
            count <= count + 1'b1;
endmodule
"#,
            )
            .expect("compile async_reset_counter");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(inputs([("clk".into(), 1), ("reset".into(), 0)]))
            .expect("step rise");
        assert_signal_eq!(outputs, "count", 1);

        let outputs = sim
            .step(inputs([("clk".into(), 0), ("reset".into(), 1)]))
            .expect("step async reset rise");
        assert_signal_eq!(outputs, "count", 0);

        let outputs = sim
            .step(inputs([("clk".into(), 1), ("reset".into(), 1)]))
            .expect("step with reset held");
        assert_signal_eq!(outputs, "count", 0);

        let outputs = sim
            .step(inputs([("clk".into(), 0), ("reset".into(), 0)]))
            .expect("release reset");
        assert_signal_eq!(outputs, "count", 0);

        let outputs = sim
            .step(inputs([("clk".into(), 1), ("reset".into(), 0)]))
            .expect("step post-reset rise");
        assert_signal_eq!(outputs, "count", 1);
    }

    #[test]
    fn load_memory_file_reads_decimal_program_file_into_child_rom() {
        let repo = repo_root();
        let design = Compiler::new()
            .add_search_path(repo.join("parts/overture"))
            .compile_file(repo.join("parts/overture/overture_cpu.sv"))
            .expect("compile overture_cpu");
        let mut sim = design.instantiate_top().expect("instantiate");
        sim.load_memory_file(
            &["fetch_unit"],
            "rom",
            repo.join("parts/overture/overture_prog_alu.txt"),
        )
        .expect("load overture program");

        assert_eq!(
            sim.read_memory_word(&["fetch_unit"], "rom", 0)
                .expect("read instruction 0"),
            bv(0x05)
        );
        assert_eq!(
            sim.read_memory_word(&["fetch_unit"], "rom", 1)
                .expect("read instruction 1"),
            bv(0x81)
        );
        assert_eq!(
            sim.read_memory_word(&["fetch_unit"], "rom", 16)
                .expect("read instruction 16"),
            bv(0x9e)
        );
    }

    #[test]
    fn eval_once_runs_vector_concatenation_assignment() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/016-Vector3.sv"))
            .expect("compile 016-Vector3");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("a".into(), 31),
                ("b".into(), 21),
                ("c".into(), 10),
                ("d".into(), 5),
                ("e".into(), 3),
                ("f".into(), 1),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "w", 253);
        assert_signal_eq!(outputs, "x", 84);
        assert_signal_eq!(outputs, "y", 81);
        assert_signal_eq!(outputs, "z", 135);
    }

    #[test]
    fn eval_once_runs_bit_reversal_concatenation() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/017-Vectorr.sv"))
            .expect("compile 017-Vectorr");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([("in".into(), 0b1101_0011)]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 0b1100_1011);
    }

    #[test]
    fn read_signal_reads_settled_child_signal_values() {
        let temp_dir = unique_temp_dir("read-hier-signal");
        let design = Compiler::new()
            .compile_str(
                temp_dir.join("top.sv"),
                concat!(
                    "module leaf(",
                    "input logic [7:0] a, ",
                    "output logic [7:0] out",
                    "); ",
                    "logic [7:0] mirrored; ",
                    "assign mirrored = a + 8'd1; ",
                    "assign out = mirrored; ",
                    "endmodule\n",
                    "module top(",
                    "input logic [7:0] in, ",
                    "output logic [7:0] out",
                    "); ",
                    "leaf u_leaf(.a(in), .out(out)); ",
                    "endmodule\n"
                ),
            )
            .expect("compile top");
        let mut sim = design.instantiate_top().expect("instantiate");
        let in_inputs = inputs([("in".into(), 5)]);
        let outputs = sim.eval_once(in_inputs.clone()).expect("eval");

        assert_signal_eq!(outputs, "out", 6);
        assert_eq!(
            sim.read_signal(&in_inputs, &["u_leaf"], "mirrored")
                .expect("read child signal"),
            BitValue::from(6_u64)
        );
    }

    #[test]
    fn eval_once_runs_sign_extension_replication() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/018-Vector4SignExtension.sv"))
            .expect("compile 018-Vector4SignExtension");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim.eval_once(inputs([("in".into(), 0x81)])).expect("eval");

        assert_signal_eq!(outputs, "out", 0xffff_ff81);
    }

    #[test]
    fn eval_once_runs_multi_expression_replication_with_sv_bit_order() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/testing/019-Vector5.sv"))
            .expect("compile 019-Vector5");
        let mut sim = design.instantiate_top().expect("instantiate");
        let outputs = sim
            .eval_once(inputs([
                ("a".into(), 1),
                ("b".into(), 0),
                ("c".into(), 1),
                ("d".into(), 0),
                ("e".into(), 1),
            ]))
            .expect("eval");

        assert_signal_eq!(outputs, "out", 22_369_621);
    }

    #[test]
    fn eval_once_runs_arbitrary_width_passthrough() {
        let design = Compiler::new()
            .compile_str(
                PathBuf::from("/virtual/top.sv"),
                concat!(
                    "module top(",
                    "input logic [191:0] inA, ",
                    "output logic [191:0] outY",
                    "); ",
                    "assign outY = inA; ",
                    "endmodule\n"
                ),
            )
            .expect("compile wide passthrough");
        let mut sim = design.instantiate_top().expect("instantiate");
        let input =
            BitValue::from_prefixed_str("0x1234567890abcdef1234567890abcdef1234567890abcdef")
                .expect("parse wide input");
        let outputs = sim
            .eval_once(BTreeMap::from([("inA".into(), input.clone())]))
            .expect("eval");

        assert_eq!(outputs.get("outY").cloned(), Some(input));
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        path.push(format!("svsim-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}
