//! Expression evaluation, lvalue resolution, and net-driver staging.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResolvedLValue {
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

pub(super) fn resolve_supported_module<'a>(
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

pub(super) fn resolve_lvalue(
    lvalue: &LValue,
    module: &ModuleSummary,
    values: &impl ValueReader,
    memories: &FxHashMap<String, MemoryState>,
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
                .to_bit_value_checked()
                .and_then(|bits| bits.to_usize_checked())
                .ok_or_else(|| Error::Resolve("memory index exceeds host limits".into()))?,
        }),
    }
}

pub(super) fn resolved_lvalue_contains_memory(lvalue: &ResolvedLValue) -> bool {
    match lvalue {
        ResolvedLValue::Signal(_)
        | ResolvedLValue::BitSelect { .. }
        | ResolvedLValue::PartSelect { .. } => false,
        ResolvedLValue::Concat(items) => items.iter().any(resolved_lvalue_contains_memory),
        ResolvedLValue::MemoryElement { .. } => true,
    }
}

pub(super) fn resolved_lvalue_width(
    lvalue: &ResolvedLValue,
    module: &ModuleSummary,
) -> Result<usize> {
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

pub(super) fn apply_resolved_lvalue(
    lvalue: &ResolvedLValue,
    value: Value,
    module: &ModuleSummary,
    values: &mut FxHashMap<String, Value>,
    memories: &mut FxHashMap<String, MemoryState>,
) -> Result<bool> {
    match lvalue {
        ResolvedLValue::Signal(name) => {
            let current = values.get_mut(name).ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                ))
            })?;
            let coerced = value.coerced_to(current.width);
            let next = Value::from_logic(coerced.logic, current.width);
            let changed = *current != next;
            *current = next;
            Ok(changed)
        }
        ResolvedLValue::Concat(items) => {
            let total_width = resolved_lvalue_width(lvalue, module)?;
            let normalized = value.coerced_to(total_width);
            let mut remaining_width = total_width;
            let mut changed = false;
            for item in items {
                let item_width = resolved_lvalue_width(item, module)?;
                remaining_width -= item_width;
                let chunk = Value::from_logic(
                    logic_slice(normalized.logic(), remaining_width, item_width),
                    item_width,
                );
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
            let bit = value.coerced_to(1).logic().bit(0);
            let next = Value::from_logic(
                logic_replace_slice(current.logic(), *index, 1, &logic_value_from_bit(bit)),
                current.width,
            );
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
            let next = Value::from_logic(
                logic_replace_slice(current.logic(), low, width, value.coerced_to(width).logic()),
                current.width,
            );
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

/// Seeds the overlay with the current value of every signal the lvalue
/// touches, so `apply_resolved_lvalue` (which mutates map entries in place)
/// sees the same starting state the old full value table provided.
pub(super) fn seed_overlay_for_lvalue(
    lvalue: &ResolvedLValue,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &[ObjectValue],
    overlay: &mut FxHashMap<String, Value>,
) {
    match lvalue {
        ResolvedLValue::Signal(name)
        | ResolvedLValue::BitSelect { signal: name, .. }
        | ResolvedLValue::PartSelect { signal: name, .. } => {
            if !overlay.contains_key(name) {
                let reader = FrameValues {
                    module,
                    state,
                    frame,
                };
                if let Some(value) = reader.read_value(name) {
                    overlay.insert(name.clone(), value);
                }
            }
        }
        ResolvedLValue::Concat(items) => {
            for item in items {
                seed_overlay_for_lvalue(item, module, state, frame, overlay);
            }
        }
        ResolvedLValue::MemoryElement { .. } => {}
    }
}

/// Frame-native equivalent of routing a child output sink through the old
/// clone-table-then-sync path: net-storage targets stage drivers, variable
/// targets are read-modified-written on the frame directly, and a variable
/// target backing a net object refreshes that object's driver list, exactly
/// as `sync_instance_values_to_frame` would have done for the touched name.
pub(super) fn apply_resolved_lvalue_to_frame(
    lvalue: &ResolvedLValue,
    value: Value,
    module: &ModuleSummary,
    state: &ModuleState,
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
    let signal_binding = |signal: &str| {
        state.signals.get(signal).copied().ok_or_else(|| {
            Error::Resolve(format!(
                "signal '{}' is not declared in '{}'",
                signal, module.name
            ))
        })
    };

    match lvalue {
        ResolvedLValue::Signal(name) => {
            let binding = signal_binding(name)?;
            if signal_storage(module, name).is_some_and(StorageKind::is_net) {
                stage_whole_signal_driver(binding, value, object_layouts, net_drivers)?;
                return Ok(false);
            }
            if object_layouts
                .get(binding.object_id)
                .is_some_and(|object| object.storage.is_net())
            {
                replace_whole_signal_driver(binding, value.clone(), object_layouts, net_drivers)?;
            }
            write_binding(binding, value, frame, object_layouts)
        }
        ResolvedLValue::BitSelect { signal, index } => {
            let binding = signal_binding(signal)?;
            if signal_storage(module, signal).is_some_and(StorageKind::is_net) {
                stage_partial_signal_driver(
                    binding,
                    *index,
                    1,
                    value.coerced_to(1),
                    object_layouts,
                    net_drivers,
                )?;
                return Ok(false);
            }
            let current = read_binding(binding, frame)?;
            if *index >= current.width {
                return Err(Error::Resolve(format!(
                    "bit select [{}] is out of range for signal '{}'",
                    index, signal
                )));
            }
            let bit = value.coerced_to(1).logic().bit(0);
            let next = Value::from_logic(
                logic_replace_slice(current.logic(), *index, 1, &logic_value_from_bit(bit)),
                current.width,
            );
            if object_layouts
                .get(binding.object_id)
                .is_some_and(|object| object.storage.is_net())
            {
                replace_whole_signal_driver(binding, next.clone(), object_layouts, net_drivers)?;
            }
            write_binding(binding, next, frame, object_layouts)
        }
        ResolvedLValue::PartSelect { signal, msb, lsb } => {
            let binding = signal_binding(signal)?;
            let low = (*msb).min(*lsb);
            let high = (*msb).max(*lsb);
            let width = high - low + 1;
            if signal_storage(module, signal).is_some_and(StorageKind::is_net) {
                stage_partial_signal_driver(
                    binding,
                    low,
                    width,
                    value.coerced_to(width),
                    object_layouts,
                    net_drivers,
                )?;
                return Ok(false);
            }
            let current = read_binding(binding, frame)?;
            if high >= current.width {
                return Err(Error::Resolve(format!(
                    "part select [{}:{}] is out of range for signal '{}'",
                    msb, lsb, signal
                )));
            }
            let next = Value::from_logic(
                logic_replace_slice(current.logic(), low, width, value.coerced_to(width).logic()),
                current.width,
            );
            if object_layouts
                .get(binding.object_id)
                .is_some_and(|object| object.storage.is_net())
            {
                replace_whole_signal_driver(binding, next.clone(), object_layouts, net_drivers)?;
            }
            write_binding(binding, next, frame, object_layouts)
        }
        ResolvedLValue::Concat(items) => {
            let total_width = resolved_lvalue_width(lvalue, module)?;
            let normalized = value.coerced_to(total_width);
            let mut remaining_width = total_width;
            let mut changed = false;
            for item in items {
                let item_width = resolved_lvalue_width(item, module)?;
                remaining_width -= item_width;
                let chunk = Value::from_logic(
                    logic_slice(normalized.logic(), remaining_width, item_width),
                    item_width,
                );
                changed |= apply_resolved_lvalue_to_frame(
                    item,
                    chunk,
                    module,
                    state,
                    frame,
                    object_layouts,
                    net_drivers,
                )?;
            }
            Ok(changed)
        }
        ResolvedLValue::MemoryElement { memory, .. } => Err(Error::Resolve(format!(
            "memory '{}' is not declared in '{}'",
            memory, module.name
        ))),
    }
}

pub(super) fn stage_signal_driver_if_net(
    signal_name: &str,
    value: Value,
    module: &ModuleSummary,
    state: &ModuleState,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    if !signal_storage(module, signal_name).is_some_and(StorageKind::is_net) {
        return Ok(());
    }
    let binding = state.signals.get(signal_name).copied().ok_or_else(|| {
        Error::Resolve(format!(
            "signal '{}' is not declared in '{}'",
            signal_name, module.name
        ))
    })?;
    stage_whole_signal_driver(binding, value, object_layouts, net_drivers)
}

pub(super) fn stage_whole_signal_driver(
    binding: SignalBinding,
    value: Value,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let logic = value.coerced_to(binding.view_width).logic;
    stage_whole_signal_logic_driver(binding, logic, object_layouts, net_drivers)
}

pub(super) fn stage_whole_signal_logic_driver(
    binding: SignalBinding,
    value: LogicValue,
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
        .coerced_to(object.width);
    stage_object_driver(binding.object_id, logic, net_drivers);
    Ok(())
}

pub(super) fn stage_partial_signal_driver(
    binding: SignalBinding,
    low: usize,
    width: usize,
    value: Value,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let logic = value.coerced_to(width).logic;
    stage_partial_signal_logic_driver(binding, low, width, logic, object_layouts, net_drivers)
}

pub(super) fn stage_partial_signal_logic_driver(
    binding: SignalBinding,
    low: usize,
    width: usize,
    value: LogicValue,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<()> {
    let object = object_layouts.get(binding.object_id).ok_or_else(|| {
        Error::Resolve(format!(
            "runtime object {} does not exist",
            binding.object_id
        ))
    })?;
    let mut bits = LogicBits::filled(object.width, LogicBit::Z);
    let value = value.coerced_to(width);
    for offset in 0..width {
        bits.set_bit(low + offset, value.bit(offset));
    }
    stage_object_driver(
        binding.object_id,
        LogicValue::new(bits, object.width),
        net_drivers,
    );
    Ok(())
}

pub(super) fn apply_or_stage_resolved_lvalue(
    lvalue: &ResolvedLValue,
    value: Value,
    module: &ModuleSummary,
    state: &ModuleState,
    values: &mut FxHashMap<String, Value>,
    memories: &mut FxHashMap<String, MemoryState>,
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &mut NetDriverTable,
) -> Result<bool> {
    match lvalue {
        ResolvedLValue::Signal(name)
            if signal_storage(module, name).is_some_and(StorageKind::is_net) =>
        {
            let binding = state.signals.get(name).copied().ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    name, module.name
                ))
            })?;
            stage_whole_signal_driver(binding, value, object_layouts, net_drivers)?;
            Ok(false)
        }
        ResolvedLValue::BitSelect { signal, index }
            if signal_storage(module, signal).is_some_and(StorageKind::is_net) =>
        {
            let binding = state.signals.get(signal).copied().ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            stage_partial_signal_driver(
                binding,
                *index,
                1,
                value.coerced_to(1),
                object_layouts,
                net_drivers,
            )?;
            Ok(false)
        }
        ResolvedLValue::PartSelect { signal, msb, lsb }
            if signal_storage(module, signal).is_some_and(StorageKind::is_net) =>
        {
            let binding = state.signals.get(signal).copied().ok_or_else(|| {
                Error::Resolve(format!(
                    "signal '{}' is not declared in '{}'",
                    signal, module.name
                ))
            })?;
            let low = (*msb).min(*lsb);
            let width = (*msb).max(*lsb) - low + 1;
            stage_partial_signal_driver(
                binding,
                low,
                width,
                value.coerced_to(width),
                object_layouts,
                net_drivers,
            )?;
            Ok(false)
        }
        ResolvedLValue::Concat(items) => {
            let total_width = resolved_lvalue_width(lvalue, module)?;
            let normalized = value.coerced_to(total_width);
            let mut remaining_width = total_width;
            let mut changed = false;
            for item in items {
                let item_width = resolved_lvalue_width(item, module)?;
                remaining_width -= item_width;
                let chunk = Value::from_logic(
                    logic_slice(normalized.logic(), remaining_width, item_width),
                    item_width,
                );
                changed |= apply_or_stage_resolved_lvalue(
                    item,
                    chunk,
                    module,
                    state,
                    values,
                    memories,
                    object_layouts,
                    net_drivers,
                )?;
            }
            Ok(changed)
        }
        _ => apply_resolved_lvalue(lvalue, value, module, values, memories),
    }
}

pub(super) fn resolve_staged_nets(
    frame: &mut [ObjectValue],
    object_layouts: &[RuntimeObjectLayout],
    net_drivers: &NetDriverTable,
) -> Result<bool> {
    let mut changed = false;

    for (object_id, object) in object_layouts.iter().enumerate() {
        let Some(kind) = object.storage.net_kind() else {
            continue;
        };
        let previous = frame.get(object_id).cloned().ok_or_else(|| {
            Error::Resolve(format!("runtime object {} has no value slot", object_id))
        })?;
        let resolved = resolve_net(
            kind,
            object.width,
            Some(&previous.logic),
            net_drivers
                .get(&object_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        )
        .map_err(|error| Error::Resolve(error.to_string()))?;
        let next = ObjectValue::from_logic(resolved);
        if frame[object_id] != next {
            frame[object_id] = next;
            changed = true;
        }
    }

    Ok(changed)
}
