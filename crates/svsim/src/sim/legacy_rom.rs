//! Legacy ROM compatibility shim.
//!
//! Contract (inherited from the pre-rewrite corpus): a module named
//! `rom_<stem>` that is a port-only wrapper — exactly one input address port
//! and one output data port, no internal logic — is treated as a ROM
//! primitive. Its contents are loaded from `<stem>.txt`, searched for in the
//! module's source directory, `<source dir>/roms/`, then `<cwd>/roms/`
//! (see [`resolve_legacy_rom_data_path`]). `validate.rs` enforces the wrapper
//! shape and file existence at compile time; the runtime drives the data port
//! combinationally from the address port each settle pass.
//!
//! This naming-convention magic contradicts the repo's stated preference for
//! explicit memory/program bindings (AGENTS.md) and is kept only for
//! compatibility with the legacy corpus. Prefer JSON-harness memory bindings
//! for new designs; do not extend this shim.

use super::*;

#[derive(Debug, Clone)]
pub(super) struct LegacyRomState {
    pub(super) addr_port: String,
    pub(super) data_port: String,
    pub(super) words: Vec<Value>,
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
    values: &mut FxHashMap<String, Value>,
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

pub(crate) fn resolve_legacy_rom_data_path(
    source_path: &Path,
    rom_name: &str,
) -> Option<std::path::PathBuf> {
    let file_name = format!("{rom_name}.txt");
    let mut candidates = Vec::new();
    if let Some(source_dir) = source_path.parent() {
        candidates.push(source_dir.join(&file_name));
        candidates.push(source_dir.join("roms").join(&file_name));
    }
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("roms").join(&file_name));
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}
