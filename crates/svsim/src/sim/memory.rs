//! Memory state, $readmem-style memory-file parsing, and the legacy ROM
//! compatibility shim (magic `rom_*` naming; see the architectural review).

use super::*;

#[derive(Debug, Clone)]
pub(super) struct MemoryState {
    pub(super) index_range: PackedRange,
    pub(super) words: Vec<Value>,
}

#[derive(Debug, Clone)]
pub(super) struct LegacyRomState {
    pub(super) addr_port: String,
    pub(super) data_port: String,
    pub(super) words: Vec<Value>,
}

impl MemoryState {
    pub(super) fn read(&self, index: usize, memory_name: &str) -> Result<Value> {
        let offset = self.index_range.index_offset(index).ok_or_else(|| {
            Error::Resolve(format!(
                "memory index [{}] is out of range for '{}'",
                index, memory_name
            ))
        })?;
        Ok(self.words[offset].clone())
    }

    pub(super) fn write(&mut self, index: usize, value: Value, memory_name: &str) -> Result<bool> {
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
        let coerced = value.coerced_to(current.width);
        let next = Value::from_logic(coerced.logic, current.width);
        let changed = *current != next;
        *current = next;
        Ok(changed)
    }
}

pub(super) fn parse_memory_file(
    path: &Path,
    word_width: usize,
    depth: usize,
) -> Result<Vec<(usize, BitValue)>> {
    let text = fs::read_to_string(path)?;
    parse_memory_text(&text, path, word_width, depth)
}

pub(super) fn parse_memory_text(
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

pub(super) fn strip_memory_comments(line: &str) -> Option<&str> {
    let mut end = line.len();
    for marker in ["//", "#"] {
        if let Some(index) = line.find(marker) {
            end = end.min(index);
        }
    }

    let trimmed = line[..end].trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(super) fn parse_memory_address(text: &str, path: &Path, line_number: usize) -> Result<usize> {
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

pub(super) fn parse_memory_value(text: &str, path: &Path, line_number: usize) -> Result<BitValue> {
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

pub(super) fn parse_prefixed_value(raw: &str) -> std::result::Result<BitValue, ParseBitValueError> {
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

pub(super) fn build_memory_table(module: &ModuleSummary) -> HashMap<String, MemoryState> {
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

pub(super) fn build_legacy_rom_state(
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

pub(super) fn apply_legacy_rom_outputs(
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
        .to_bit_value_checked()
        .and_then(|bits| bits.to_usize_checked())
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
