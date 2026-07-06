//! $readmem-style memory-file parsing and memory table construction.

use super::*;

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
