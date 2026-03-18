use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use crate::bit_value::{BitValue, ParseBitValueError};
use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::hir::{
    AssignmentKind, BinaryOp, Expr, HirDesign, LValue, ModuleInstanceSummary, ModuleSummary,
    NumericLiteral, PackedRange, PortDirection, ProcBlockKind, Stmt, UnaryOp, expr_to_lvalue,
};
use crate::validate::resolve_legacy_rom_data_path;
use crate::width::{expr_width, mask, minimum_width, shift_left_bits, shift_right_bits};

#[derive(Debug, Clone)]
pub struct SimulationSession {
    design: CompiledDesign,
    state: ModuleState,
}

#[derive(Debug, Clone)]
struct ModuleState {
    module_name: String,
    persisted: HashMap<String, Value>,
    memories: HashMap<String, MemoryState>,
    legacy_rom: Option<LegacyRomState>,
    children: Vec<ChildState>,
}

#[derive(Debug, Clone)]
struct ChildState {
    state: Box<ModuleState>,
}

#[derive(Debug, Clone)]
struct InstanceEvalCache {
    inputs: BTreeMap<String, BitValue>,
    outputs: HashMap<String, Value>,
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
        let top_module = design
            .top_module()
            .expect("compiled designs always carry a top module");
        let mut stack = Vec::new();
        let state = instantiate_module_state(design.hir(), top_module, &mut stack)?;
        Ok(Self { design, state })
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
        let module_state = resolve_instance_path_mut(hir, &mut self.state, instance_path)?;
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
        let module_state = resolve_instance_path_mut(hir, &mut self.state, instance_path)?;
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
        let module_state = resolve_instance_path(hir, &self.state, instance_path)?;
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

    pub fn eval_once(
        &mut self,
        inputs: BTreeMap<String, BitValue>,
    ) -> Result<BTreeMap<String, BitValue>> {
        let module = top_module(self.design.hir(), self.top_module())?;
        let mut stack = Vec::new();
        let values = settle_module(self.design.hir(), module, &self.state, &inputs, &mut stack)?;
        Ok(collect_outputs(module, &values))
    }

    pub fn step(
        &mut self,
        inputs: BTreeMap<String, BitValue>,
    ) -> Result<BTreeMap<String, BitValue>> {
        let module = top_module(self.design.hir(), self.top_module())?;
        let mut stack = Vec::new();
        step_module(self.design.hir(), &mut self.state, &inputs, &mut stack)?;

        let mut settle_stack = Vec::new();
        let values = settle_module(
            self.design.hir(),
            module,
            &self.state,
            &inputs,
            &mut settle_stack,
        )?;
        Ok(collect_outputs(module, &values))
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
}

impl Value {
    fn new(bits: BitValue, width: usize) -> Self {
        let width = width.max(1);
        Self {
            bits: bits.truncate(width),
            width,
        }
    }

    fn coerced_to(&self, width: usize) -> Self {
        Self::new(self.normalized_bits(), width)
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
    module_name: &str,
    stack: &mut Vec<String>,
) -> Result<ModuleState> {
    if stack.iter().any(|name| name == module_name) {
        return Err(Error::Unsupported(format!(
            "recursive instantiation detected at {} -> {}",
            stack.join(" -> "),
            module_name
        )));
    }

    let module = resolve_supported_module(hir, module_name)?;
    stack.push(module_name.to_owned());

    let mut children = Vec::with_capacity(module.instantiations.len());
    for instance in &module.instantiations {
        children.push(ChildState {
            state: Box::new(instantiate_module_state(hir, &instance.module_name, stack)?),
        });
    }

    stack.pop();
    Ok(ModuleState {
        module_name: module_name.to_owned(),
        persisted: build_persisted_signal_table(module),
        memories: build_memory_table(module),
        legacy_rom: build_legacy_rom_state(hir, module)?,
        children,
    })
}

fn settle_module(
    hir: &HirDesign,
    module: &ModuleSummary,
    state: &ModuleState,
    inputs: &BTreeMap<String, BitValue>,
    stack: &mut Vec<String>,
) -> Result<HashMap<String, Value>> {
    if stack.iter().any(|name| name == &state.module_name) {
        return Err(Error::Unsupported(format!(
            "recursive combinational instantiation detected at {} -> {}",
            stack.join(" -> "),
            state.module_name
        )));
    }

    let mut values = build_signal_table(module, inputs, &state.persisted)?;
    if let Some(legacy_rom) = &state.legacy_rom {
        apply_legacy_rom_outputs(module, &mut values, legacy_rom)?;
        return Ok(values);
    }
    let max_iterations = ((module.continuous_assignments.len()
        + module.proc_blocks.len()
        + module.instantiations.len()
        + values.len())
    .max(1))
        * 8;
    let mut instance_caches = vec![None; module.instantiations.len()];

    stack.push(state.module_name.clone());
    let mut converged = false;
    for _ in 0..max_iterations {
        let mut changed = false;

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

        for ((instance, child_state), cache) in module
            .instantiations
            .iter()
            .zip(&state.children)
            .zip(instance_caches.iter_mut())
        {
            changed |= evaluate_instance(
                hir,
                module,
                instance,
                child_state.state.as_ref(),
                &mut values,
                &state.memories,
                stack,
                cache,
            )?;
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

    Ok(values)
}

fn step_module(
    hir: &HirDesign,
    state: &mut ModuleState,
    inputs: &BTreeMap<String, BitValue>,
    stack: &mut Vec<String>,
) -> Result<()> {
    let module = resolve_supported_module(hir, &state.module_name)?;
    let pre_values = settle_module(hir, module, state, inputs, stack)?;

    for (instance, child_state) in module.instantiations.iter().zip(state.children.iter_mut()) {
        let child = resolve_supported_module(hir, &instance.module_name)?;
        let child_inputs =
            build_child_inputs(module, child, instance, &pre_values, &state.memories)?;
        step_module(hir, child_state.state.as_mut(), &child_inputs, stack)?;
    }

    let mut staged = state.persisted.clone();
    let mut staged_memories = state.memories.clone();
    for block in &module.proc_blocks {
        match &block.kind {
            ProcBlockKind::AlwaysComb => {}
            ProcBlockKind::AlwaysFf { clock } => {
                let clock_value = pre_values.get(clock).cloned().ok_or_else(|| {
                    Error::Resolve(format!(
                        "clock '{}' is not declared in '{}'",
                        clock, module.name
                    ))
                })?;
                if clock_value.truthy() {
                    let mut exec_values = pre_values.clone();
                    let mut exec_memories = state.memories.clone();
                    execute_sequential_stmt(
                        &block.body,
                        module,
                        &mut exec_values,
                        &mut exec_memories,
                        &mut staged,
                        &mut staged_memories,
                    )?;
                }
            }
        }
    }

    state.persisted = staged;
    state.memories = staged_memories;
    Ok(())
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
                apply_resolved_lvalue(&target, value, module, current_values, memories)?;
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

fn build_persisted_signal_table(module: &ModuleSummary) -> HashMap<String, Value> {
    let mut values = HashMap::new();

    for port in &module.ports {
        values.insert(port.name.clone(), Value::zero(port.width()));
    }
    for signal in &module.signals {
        values.insert(signal.name.clone(), Value::zero(signal.width()));
    }
    // Parameters are placeholder-initialized here; they get their true values
    // in build_signal_table where expression evaluation is available.
    for param in &module.parameters {
        values.insert(param.name.clone(), Value::zero(param.width()));
    }

    values
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

fn build_signal_table(
    module: &ModuleSummary,
    inputs: &BTreeMap<String, BitValue>,
    persisted: &HashMap<String, Value>,
) -> Result<HashMap<String, Value>> {
    let mut values = HashMap::new();

    for port in &module.ports {
        let value = if matches!(port.direction, PortDirection::Input) {
            Value::new(
                inputs
                    .get(&port.name)
                    .cloned()
                    .unwrap_or_else(BitValue::zero),
                port.width(),
            )
        } else {
            persisted
                .get(&port.name)
                .cloned()
                .unwrap_or_else(|| Value::zero(port.width()))
        };
        values.insert(port.name.clone(), value);
    }

    for signal in &module.signals {
        values.insert(
            signal.name.clone(),
            persisted
                .get(&signal.name)
                .cloned()
                .unwrap_or_else(|| Value::zero(signal.width())),
        );
    }

    // Evaluate parameter defaults in declaration order so later parameters can
    // reference earlier ones.
    let empty_memories = HashMap::new();
    for param in &module.parameters {
        let value = eval_expr(&param.default_value, module, &values, &empty_memories)?;
        let coerced = value.coerced_to(param.width());
        values.insert(param.name.clone(), coerced);
    }

    for name in inputs.keys() {
        if module.port(name).is_none() {
            return Err(Error::Resolve(format!(
                "input '{}' does not match any port on module '{}'",
                name, module.name
            )));
        }
    }

    Ok(values)
}

fn build_child_inputs(
    parent: &ModuleSummary,
    child: &ModuleSummary,
    instance: &ModuleInstanceSummary,
    parent_values: &HashMap<String, Value>,
    parent_memories: &HashMap<String, MemoryState>,
) -> Result<BTreeMap<String, BitValue>> {
    let mut child_inputs = BTreeMap::new();

    for port in child
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Input))
    {
        let Some(connection) = find_connection(instance, &port.name) else {
            continue;
        };
        // Cache keys should reflect the child-visible port value, not the raw parent expression.
        let value = eval_expr(&connection.expr, parent, parent_values, parent_memories)?
            .coerced_to(port.width());
        child_inputs.insert(port.name.clone(), value.normalized_bits());
    }

    Ok(child_inputs)
}

fn evaluate_instance(
    hir: &HirDesign,
    parent: &ModuleSummary,
    instance: &ModuleInstanceSummary,
    child_state: &ModuleState,
    values: &mut HashMap<String, Value>,
    memories: &HashMap<String, MemoryState>,
    stack: &mut Vec<String>,
    cache: &mut Option<InstanceEvalCache>,
) -> Result<bool> {
    let child = resolve_supported_module(hir, &instance.module_name).map_err(|_| {
        Error::Resolve(format!(
            "instance '{}' references missing module '{}'",
            instance.instance_name, instance.module_name
        ))
    })?;

    let child_inputs = build_child_inputs(parent, child, instance, values, memories)?;
    let needs_refresh = cache
        .as_ref()
        .is_none_or(|cached| cached.inputs != child_inputs);
    if needs_refresh {
        let child_values = settle_module(hir, child, child_state, &child_inputs, stack)?;
        *cache = Some(InstanceEvalCache {
            inputs: child_inputs,
            outputs: child_values,
        });
    }
    let child_values = &cache
        .as_ref()
        .expect("instance cache is initialized before applying outputs")
        .outputs;
    let mut changed = false;

    for port in child
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output))
    {
        let Some(connection) = find_connection(instance, &port.name) else {
            continue;
        };
        let lvalue = expr_to_lvalue(&connection.expr).ok_or_else(|| {
            Error::Unsupported(format!(
                "instance '{}' connects output port '{}' to a non-lvalue expression",
                instance.instance_name, port.name
            ))
        })?;
        let value = child_values
            .get(&port.name)
            .cloned()
            .unwrap_or_else(|| Value::zero(port.width()))
            .coerced_to(port.width());
        let target = resolve_lvalue(&lvalue, parent, values, memories)?;
        let mut no_memories = HashMap::new();
        changed |= apply_resolved_lvalue(&target, value, parent, values, &mut no_memories)?;
    }

    Ok(changed)
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
    values: &HashMap<String, Value>,
) -> BTreeMap<String, BitValue> {
    let mut outputs = BTreeMap::new();

    for port in module
        .ports
        .iter()
        .filter(|port| matches!(port.direction, PortDirection::Output))
    {
        let value = values
            .get(&port.name)
            .cloned()
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

fn find_connection<'a>(
    instance: &'a ModuleInstanceSummary,
    port_name: &str,
) -> Option<&'a crate::hir::NamedPortConnection> {
    instance
        .connections
        .iter()
        .find(|connection| connection.port_name == port_name)
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
                UnaryOp::BitNot => Ok(Value::new(
                    value.normalized_bits().bitnot_with_width(value.width),
                    value.width,
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
            }
        }
        Expr::Binary { left, op, right } => {
            let left = eval_expr(left, module, values, memories)?;
            let right = eval_expr(right, module, values, memories)?;
            let (bits, width) = match op {
                BinaryOp::BitAnd => (
                    left.normalized_bits().bitand(&right.normalized_bits()),
                    left.width.max(right.width),
                ),
                BinaryOp::BitOr => (
                    left.normalized_bits().bitor(&right.normalized_bits()),
                    left.width.max(right.width),
                ),
                BinaryOp::BitXor => (
                    left.normalized_bits().bitxor(&right.normalized_bits()),
                    left.width.max(right.width),
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
                BinaryOp::LogicalAnd => (BitValue::from(left.truthy() && right.truthy()), 1),
                BinaryOp::LogicalOr => (BitValue::from(left.truthy() || right.truthy()), 1),
                BinaryOp::Eq => (BitValue::from(values_equal(&left, &right)), 1),
                BinaryOp::NotEq => (BitValue::from(!values_equal(&left, &right)), 1),
                BinaryOp::Lt => (
                    BitValue::from(left.normalized_bits() < right.normalized_bits()),
                    1,
                ),
                BinaryOp::LtEq => (
                    BitValue::from(left.normalized_bits() <= right.normalized_bits()),
                    1,
                ),
                BinaryOp::Gt => (
                    BitValue::from(left.normalized_bits() > right.normalized_bits()),
                    1,
                ),
                BinaryOp::GtEq => (
                    BitValue::from(left.normalized_bits() >= right.normalized_bits()),
                    1,
                ),
                BinaryOp::Add => (
                    left.normalized_bits()
                        .wrapping_add(&right.normalized_bits(), left.width.max(right.width)),
                    left.width.max(right.width),
                ),
                BinaryOp::Sub => (
                    left.normalized_bits()
                        .wrapping_sub(&right.normalized_bits(), left.width.max(right.width)),
                    left.width.max(right.width),
                ),
                BinaryOp::Mul => (
                    left.normalized_bits()
                        .wrapping_mul(&right.normalized_bits(), left.width.max(right.width)),
                    left.width.max(right.width),
                ),
            };
            Ok(Value::new(bits, width))
        }
        Expr::Ternary {
            cond,
            when_true,
            when_false,
        } => {
            let result_width = expr_width(when_true, module)?.max(expr_width(when_false, module)?);
            if eval_expr(cond, module, values, memories)?.truthy() {
                Ok(Value::new(
                    eval_expr(when_true, module, values, memories)?.normalized_bits(),
                    result_width,
                ))
            } else {
                Ok(Value::new(
                    eval_expr(when_false, module, values, memories)?.normalized_bits(),
                    result_width,
                ))
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
    left.normalized_bits() == right.normalized_bits()
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
    hir: &'a HirDesign,
    state: &'a ModuleState,
    instance_path: &[&str],
) -> Result<&'a ModuleState> {
    let Some((segment, rest)) = instance_path.split_first() else {
        return Ok(state);
    };
    let module = resolve_supported_module(hir, &state.module_name)?;
    let child_index = module
        .instantiations
        .iter()
        .position(|instance| instance.instance_name == *segment)
        .ok_or_else(|| {
            Error::Resolve(format!(
                "instance path '{}' does not exist under module '{}'",
                instance_path.join("."),
                module.name
            ))
        })?;
    resolve_instance_path(hir, state.children[child_index].state.as_ref(), rest)
}

fn resolve_instance_path_mut<'a>(
    hir: &HirDesign,
    state: &'a mut ModuleState,
    instance_path: &[&str],
) -> Result<&'a mut ModuleState> {
    let Some((segment, rest)) = instance_path.split_first() else {
        return Ok(state);
    };
    let module = resolve_supported_module(hir, &state.module_name)?;
    let child_index = module
        .instantiations
        .iter()
        .position(|instance| instance.instance_name == *segment)
        .ok_or_else(|| {
            Error::Resolve(format!(
                "instance path '{}' does not exist under module '{}'",
                instance_path.join("."),
                module.name
            ))
        })?;
    resolve_instance_path_mut(hir, state.children[child_index].state.as_mut(), rest)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("enable".into(), 1),
                ("data".into(), 0x5a),
            ]))
            .expect("step");
        assert_signal_eq!(outputs, "q", 0x5a);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("enable".into(), 0),
                ("data".into(), 0xff),
            ]))
            .expect("hold");
        assert_signal_eq!(outputs, "q", 0x5a);
    }

    #[test]
    fn step_runs_counter_module() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/counter8.sv"))
            .expect("compile counter8");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 1),
                ("enable".into(), 0),
            ]))
            .expect("reset");
        assert_signal_eq!(outputs, "count", 0);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("enable".into(), 1),
            ]))
            .expect("increment");
        assert_signal_eq!(outputs, "count", 1);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("enable".into(), 0),
            ]))
            .expect("hold");
        assert_signal_eq!(outputs, "count", 1);
    }

    #[test]
    fn step_runs_hierarchical_regfile() {
        let repo = repo_root();
        let design = Compiler::new()
            .compile_file(repo.join("parts/basic/regfile_8x8.sv"))
            .expect("compile regfile_8x8");
        let mut sim = design.instantiate_top().expect("instantiate");

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("write_en".into(), 1),
                ("write_addr".into(), 3),
                ("write_data".into(), 0x42),
                ("read_addr1".into(), 3),
                ("read_addr2".into(), 0),
            ]))
            .expect("write r3");
        assert_signal_eq!(outputs, "read_data1", 0x42);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("write_en".into(), 1),
                ("write_addr".into(), 1),
                ("write_data".into(), 0x99),
                ("read_addr1".into(), 1),
                ("read_addr2".into(), 3),
            ]))
            .expect("write r1");
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

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 1),
                ("run".into(), 0),
                ("jump_en".into(), 0),
                ("jump_addr".into(), 0),
            ]))
            .expect("reset");
        assert_signal_eq!(outputs, "pc", 0);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
                ("jump_en".into(), 0),
                ("jump_addr".into(), 0),
            ]))
            .expect("increment");
        assert_signal_eq!(outputs, "pc", 1);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
                ("jump_en".into(), 1),
                ("jump_addr".into(), 10),
            ]))
            .expect("jump");
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

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 1),
                ("run".into(), 0),
            ]))
            .expect("reset");
        assert_signal_eq!(outputs, "pc", 0);
        assert_signal_eq!(outputs, "acc", 0);
        assert_signal_eq!(outputs, "ram_out", 0);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
            ]))
            .expect("load immediate");
        assert_signal_eq!(outputs, "pc", 1);
        assert_signal_eq!(outputs, "acc", 3);
        assert_signal_eq!(outputs, "ram_out", 0);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
            ]))
            .expect("add immediate");
        assert_signal_eq!(outputs, "pc", 2);
        assert_signal_eq!(outputs, "acc", 5);
        assert_signal_eq!(outputs, "ram_out", 0);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
            ]))
            .expect("store acc");
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

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 1),
                ("run".into(), 0),
                ("in_port".into(), 0),
            ]))
            .expect("reset");
        assert_signal_eq!(outputs, "pc", 0);

        let outputs = sim
            .step(inputs([
                ("clk".into(), 1),
                ("reset".into(), 0),
                ("run".into(), 1),
                ("in_port".into(), 0),
            ]))
            .expect("execute immediate");
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
