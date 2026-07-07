//! The public simulation session API and the settle/step scheduler.

use super::*;

#[derive(Debug, Clone)]
pub struct SimulationSession {
    pub(super) design: CompiledDesign,
    pub(super) objects: Vec<RuntimeObjectLayout>,
    pub(super) persisted: Vec<ObjectValue>,
    pub(super) state: ModuleState,
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
            &mut objects,
            &mut stack,
        )?;
        let persisted = objects
            .iter()
            .map(|object| ObjectValue::zero(object.width))
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
        words: &[LogicValue],
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
                Value::from_logic(word.clone(), memory_decl.element_width()),
                memory_name,
            )?;
        }

        Ok(())
    }

    pub fn load_memory_words_2state(
        &mut self,
        instance_path: &[&str],
        memory_name: &str,
        words: &[BitValue],
    ) -> Result<()> {
        let logic_words = words
            .iter()
            .cloned()
            .map(LogicValue::from)
            .collect::<Vec<_>>();
        self.load_memory_words(instance_path, memory_name, &logic_words)
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
    ) -> Result<LogicValue> {
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
        Ok(memory_state.read(index, memory_name)?.logic().clone())
    }

    pub fn read_memory_word_2state(
        &self,
        instance_path: &[&str],
        memory_name: &str,
        index: usize,
    ) -> Result<BitValue> {
        let logic = self.read_memory_word(instance_path, memory_name, index)?;
        logic_to_public_bit_value(&logic, format!("memory '{}' word {}", memory_name, index))
    }

    pub fn read_signal(
        &self,
        inputs: &BTreeMap<String, LogicValue>,
        instance_path: &[&str],
        signal_name: &str,
    ) -> Result<LogicValue> {
        let hir = self.design.hir();
        let module = top_module(hir, self.top_module())?;
        let mut frame =
            seed_runtime_frame(module, &self.state, &self.persisted, &self.objects, inputs)?;
        let mut stack = Vec::new();
        settle_module(
            hir,
            module,
            &self.state,
            &mut frame,
            &self.objects,
            Some(inputs),
            &mut stack,
        )?;

        let module_state = resolve_instance_path(&self.state, instance_path)?;
        let instance_module = resolve_supported_module(hir, &module_state.module_name)?;
        let binding = module_state
            .signals
            .get(signal_name)
            .copied()
            .ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal_name, instance_module.name
                ))
            })?;
        read_binding_logic(binding, &frame)
    }

    pub fn read_signal_2state(
        &self,
        inputs: &BTreeMap<String, BitValue>,
        instance_path: &[&str],
        signal_name: &str,
    ) -> Result<BitValue> {
        let logic_inputs = logic_inputs_from_public_bits(inputs);
        let logic = self.read_signal(&logic_inputs, instance_path, signal_name)?;
        logic_to_public_bit_value(&logic, format!("signal '{}'", signal_name))
    }

    pub fn eval_once(
        &mut self,
        inputs: BTreeMap<String, LogicValue>,
    ) -> Result<BTreeMap<String, LogicValue>> {
        let module = top_module(self.design.hir(), self.top_module())?;
        let mut frame =
            seed_runtime_frame(module, &self.state, &self.persisted, &self.objects, &inputs)?;
        let mut stack = Vec::new();
        settle_module(
            self.design.hir(),
            module,
            &self.state,
            &mut frame,
            &self.objects,
            Some(&inputs),
            &mut stack,
        )?;
        collect_outputs_logic(module, &self.state, &frame)
    }

    pub fn eval_once_2state(
        &mut self,
        inputs: BTreeMap<String, BitValue>,
    ) -> Result<BTreeMap<String, BitValue>> {
        let logic_outputs = self.eval_once(logic_inputs_from_public_bits(&inputs))?;
        logic_outputs_to_public_bits(logic_outputs)
    }

    pub fn step(
        &mut self,
        inputs: BTreeMap<String, LogicValue>,
    ) -> Result<BTreeMap<String, LogicValue>> {
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
            Some(&inputs),
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
            Some(&inputs),
            &mut post_settle_stack,
        )?;
        collect_outputs_logic(module, &self.state, &post_frame)
    }

    pub fn step_2state(
        &mut self,
        inputs: BTreeMap<String, BitValue>,
    ) -> Result<BTreeMap<String, BitValue>> {
        let logic_outputs = self.step(logic_inputs_from_public_bits(&inputs))?;
        logic_outputs_to_public_bits(logic_outputs)
    }
}

fn settle_module(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    inputs: Option<&BTreeMap<String, LogicValue>>,
    stack: &mut Vec<String>,
) -> Result<()> {
    // One confirming pass on top of the budget (the last productive pass
    // still reports a change), with a floor for degenerate tiny modules
    // whose element count under-counts four-state settling (an undriven
    // pulled net changes once before it can be confirmed stable).
    let max_iterations = (settle_iteration_budget(hir, state)? + 1).max(16);
    let mut converged = false;
    let mut iterations_used = 0usize;
    for _ in 0..max_iterations {
        iterations_used += 1;
        let mut net_drivers = NetDriverTable::new();
        let pass_changed = settle_module_pass(
            hir,
            module,
            state,
            frame,
            object_layouts,
            inputs,
            &mut net_drivers,
            stack,
        )?;
        let nets_changed = resolve_staged_nets(frame, object_layouts, &net_drivers)?;

        if !(pass_changed || nets_changed) {
            converged = true;
            break;
        }
    }
    // Re-measure with `SVSIM_SETTLE_STATS=1` when the corpus grows; see
    // `settle_iteration_budget` for the last measured headroom.
    if std::env::var_os("SVSIM_SETTLE_STATS").is_some() {
        eprintln!(
            "SETTLE_STATS module={} used={} budget={}",
            module.name, iterations_used, max_iterations
        );
    }

    if !converged {
        return Err(Error::Unsupported(format!(
            "combinational evaluation did not converge for module '{}'",
            module.name
        )));
    }

    Ok(())
}

/// Upper bound on productive settle iterations: the recursive sum of
/// assigns, proc blocks, children, and signals — a generous
/// over-approximation of the longest combinational dependency chain.
/// Measured 2026-07-06 across 48,069 settle calls over the full corpus
/// (`SVSIM_SETTLE_STATS=1`), the deepest design (`adder_cs_64bit`)
/// converged in 12 iterations against a budget of 69,860; the historical
/// ×8 multiplier on top of this sum was dropped as unfounded. Converging
/// designs exit early, so the budget only bounds how long an oscillating
/// design runs before erroring.
fn settle_iteration_budget(hir: &HirDesign, state: &ModuleState) -> Result<usize> {
    let module = resolve_supported_module(hir, &state.module_name)?;
    let mut budget = module.continuous_assignments.len()
        + module.proc_blocks.len()
        + state.children.len()
        + state.signals.len()
        + usize::from(state.legacy_rom.is_some());

    for child in &state.children {
        budget += settle_iteration_budget(hir, child.state.as_ref())?;
    }

    Ok(budget.max(1))
}

fn settle_module_pass(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    inputs: Option<&BTreeMap<String, LogicValue>>,
    net_drivers: &mut NetDriverTable,
    stack: &mut Vec<String>,
) -> Result<bool> {
    if stack.iter().any(|name| name == &state.module_name) {
        return Err(Error::Unsupported(format!(
            "recursive combinational instantiation detected at {} -> {}",
            stack.join(" -> "),
            state.module_name
        )));
    }

    stack.push(state.module_name.clone());
    let mut changed = false;

    if let Some(inputs) = inputs {
        changed |=
            apply_external_inputs(module, state, frame, object_layouts, inputs, net_drivers)?;
    }

    if let Some(legacy_rom) = &state.legacy_rom {
        let mut values = build_instance_value_table(module, state, frame)?;
        apply_legacy_rom_outputs(module, &mut values, legacy_rom)?;
        if let Some(value) = values.get(&legacy_rom.data_port).cloned() {
            stage_signal_driver_if_net(
                &legacy_rom.data_port,
                value,
                module,
                state,
                object_layouts,
                net_drivers,
            )?;
        }
        changed |= sync_instance_values_to_frame(
            module,
            state,
            &values,
            frame,
            object_layouts,
            net_drivers,
            false,
            true,
        )?;
        stack.pop();
        return Ok(changed);
    }

    let mut overlay: HashMap<String, Value> = HashMap::new();

    for assign in &module.continuous_assignments {
        let (value, target) = {
            let reader = OverlayValues {
                module,
                state,
                frame,
                overlay: &overlay,
            };
            let value = eval_expr(&assign.expr, module, &reader, &state.memories)?;
            let target = resolve_lvalue(&assign.target, module, &reader, &state.memories)?;
            (value, target)
        };
        if resolved_lvalue_contains_memory(&target) {
            return Err(Error::Unsupported(
                "continuous assignments to memory elements are not supported".into(),
            ));
        }
        seed_overlay_for_lvalue(&target, module, state, frame, &mut overlay);
        let mut no_memories = HashMap::new();
        // Overlay-level change flags are transient: a default-then-override
        // sequence reports "changed" on every pass even at steady state.
        // Convergence listens to `commit_overlay_to_frame` below, which
        // compares each dirty name's final value against the frame.
        apply_or_stage_resolved_lvalue(
            &target,
            value,
            module,
            state,
            &mut overlay,
            &mut no_memories,
            object_layouts,
            net_drivers,
        )?;
    }

    for block in &module.proc_blocks {
        execute_proc_block(
            &block.kind,
            &block.body,
            module,
            state,
            frame,
            &mut overlay,
            &state.memories,
        )?;
    }

    changed |= commit_overlay_to_frame(state, &overlay, frame, object_layouts, net_drivers)?;

    for child_state in &state.children {
        let child = resolve_supported_module(hir, &child_state.state.module_name)?;
        let drove_inputs = drive_child_inputs(
            module,
            state,
            child_state,
            &state.memories,
            frame,
            object_layouts,
            net_drivers,
        )?;
        changed |= drove_inputs;

        let child_changed = settle_module_pass(
            hir,
            child,
            child_state.state.as_ref(),
            frame,
            object_layouts,
            None,
            net_drivers,
            stack,
        )?;
        changed |= child_changed;

        let applied_outputs = apply_child_output_sinks(
            module,
            state,
            child_state,
            &state.memories,
            frame,
            object_layouts,
            net_drivers,
        )?;
        changed |= applied_outputs;
    }

    stack.pop();
    Ok(changed)
}

fn step_module(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &mut ModuleState,
    pre_frame: &[ObjectValue],
    next_objects: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    _stack: &mut Vec<String>,
) -> Result<()> {
    let pre_values = build_instance_value_table(module, state, pre_frame)?;

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

    let mut staged = build_instance_value_table(module, state, next_objects)?;
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

    let mut no_net_drivers = NetDriverTable::new();
    sync_instance_values_to_frame(
        module,
        state,
        &staged,
        next_objects,
        object_layouts,
        &mut no_net_drivers,
        true,
        false,
    )?;
    state.memories = staged_memories;
    state.previous_clocks = sampled_clocks;
    Ok(())
}

fn apply_fixed_binding_drive(
    binding: SignalBinding,
    value: Value,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
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
    if object.storage.is_net() {
        stage_object_driver(binding.object_id, logic, net_drivers);
        return Ok(false);
    }
    write_binding_logic(binding, logic, frame, object_layouts)
}

fn apply_external_inputs(
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    inputs: &BTreeMap<String, LogicValue>,
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
    let mut changed = false;

    for port in module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input | PortDirection::Inout))
    {
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        let provided = inputs.get(&port.name).cloned();
        if matches!(port.direction, PortDirection::Inout) && provided.is_none() {
            // Omitted inout = harness is not driving. Don't stage anything;
            // internal drivers alone determine the resolved value.
            continue;
        }
        let default_value = match port.direction {
            PortDirection::Inout => LogicValue::all_z(port.width()),
            _ => LogicValue::zero(port.width()),
        };
        let value = Value::from_logic(provided.unwrap_or(default_value), port.width());
        changed |= apply_fixed_binding_drive(binding, value, frame, object_layouts, net_drivers)?;
    }

    Ok(changed)
}

fn drive_child_inputs(
    parent_module: &ModuleSummary,
    parent_state: &ModuleState,
    child_state: &ChildState,
    parent_memories: &HashMap<String, MemoryState>,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
    let mut changed = false;

    for driver in &child_state.input_drivers {
        let parent_values = FrameValues {
            module: parent_module,
            state: parent_state,
            frame,
        };
        let value = eval_expr(&driver.expr, parent_module, &parent_values, parent_memories)?;
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
        changed |= apply_fixed_binding_drive(binding, value, frame, object_layouts, net_drivers)?;
    }

    Ok(changed)
}

fn apply_child_output_sinks(
    parent_module: &ModuleSummary,
    parent_state: &ModuleState,
    child_state: &ChildState,
    parent_memories: &HashMap<String, MemoryState>,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
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
        let value = read_binding(binding, frame)?;
        let target = {
            let reader = FrameValues {
                module: parent_module,
                state: parent_state,
                frame,
            };
            resolve_lvalue(&sink.target, parent_module, &reader, parent_memories)?
        };
        changed |= apply_resolved_lvalue_to_frame(
            &target,
            value,
            parent_module,
            parent_state,
            frame,
            object_layouts,
            net_drivers,
        )?;
    }

    Ok(changed)
}

fn execute_proc_block(
    kind: &ProcBlockKind,
    body: &Stmt,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[ObjectValue],
    overlay: &mut HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<bool> {
    match kind {
        ProcBlockKind::AlwaysComb => {
            execute_comb_stmt(body, module, state, frame, overlay, memories)
        }
        ProcBlockKind::AlwaysFf { .. } => Ok(false),
    }
}

fn execute_comb_stmt(
    stmt: &Stmt,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[ObjectValue],
    overlay: &mut HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
) -> Result<bool> {
    match stmt {
        Stmt::Empty => Ok(false),
        Stmt::Block(statements) => {
            let mut changed = false;
            for statement in statements {
                changed |= execute_comb_stmt(statement, module, state, frame, overlay, memories)?;
            }
            Ok(changed)
        }
        Stmt::Assign { kind, target, expr } => match kind {
            AssignmentKind::Blocking => {
                let (value, target) = {
                    let reader = OverlayValues {
                        module,
                        state,
                        frame,
                        overlay,
                    };
                    let value = eval_expr(expr, module, &reader, memories)?;
                    let target = resolve_lvalue(target, module, &reader, memories)?;
                    (value, target)
                };
                if resolved_lvalue_contains_memory(&target) {
                    return Err(Error::Unsupported(
                        "memory element assignments are only supported inside `always_ff` blocks"
                            .into(),
                    ));
                }
                seed_overlay_for_lvalue(&target, module, state, frame, overlay);
                let mut no_memories = HashMap::new();
                apply_resolved_lvalue(&target, value, module, overlay, &mut no_memories)
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
            let truth = {
                let reader = OverlayValues {
                    module,
                    state,
                    frame,
                    overlay,
                };
                eval_expr(cond, module, &reader, memories)?.truthiness()
            };
            if matches!(truth, LogicTruth::True) {
                execute_comb_stmt(then_branch, module, state, frame, overlay, memories)
            } else if let Some(else_branch) = else_branch {
                execute_comb_stmt(else_branch, module, state, frame, overlay, memories)
            } else {
                Ok(false)
            }
        }
        Stmt::Case {
            expr,
            items,
            default,
        } => {
            let value = {
                let reader = OverlayValues {
                    module,
                    state,
                    frame,
                    overlay,
                };
                eval_expr(expr, module, &reader, memories)?
            };
            for item in items {
                for pattern in &item.patterns {
                    let matched = {
                        let reader = OverlayValues {
                            module,
                            state,
                            frame,
                            overlay,
                        };
                        values_case_equal(&value, &eval_expr(pattern, module, &reader, memories)?)
                    };
                    if matched {
                        return execute_comb_stmt(
                            &item.body, module, state, frame, overlay, memories,
                        );
                    }
                }
            }
            if let Some(default) = default {
                execute_comb_stmt(default, module, state, frame, overlay, memories)
            } else {
                Ok(false)
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
            if matches!(
                eval_expr(cond, module, current_values, memories)?.truthiness(),
                LogicTruth::True
            ) {
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
                    if values_case_equal(
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

fn collect_outputs_logic(
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[ObjectValue],
) -> Result<BTreeMap<String, LogicValue>> {
    let mut outputs = BTreeMap::new();

    for port in module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output | PortDirection::Inout))
    {
        let binding = state.signals.get(&port.name).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                port.name, module.name
            ))
        })?;
        outputs.insert(port.name.clone(), read_binding_logic(binding, frame)?);
    }

    Ok(outputs)
}
