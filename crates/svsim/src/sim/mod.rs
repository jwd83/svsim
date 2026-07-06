//! Structural runtime for compiled designs: combinational settle,
//! sequential stepping, memory loading, and signal inspection.
//!
//! Split by responsibility: `value` (four-state values and primitive ops),
//! `eval` (expression/lvalue evaluation and driver staging), `state`
//! (hierarchical module state and bindings), `memory` (memory files and the
//! legacy ROM shim), `session` (public API and scheduler).

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
use crate::logic_ops::{
    logic_bit_and, logic_bit_not, logic_bit_or, logic_bit_xor, logic_replace_slice,
    logic_sign_extend, logic_slice, logic_value_from_bit,
};
use crate::logic_value::{LogicBit, LogicBits, LogicValue};
use crate::net_resolve::{DriveStrengthPair, NetDriver, resolve_net};
use crate::validate::resolve_legacy_rom_data_path;
use crate::width::{expr_width, minimum_width};

mod eval;
mod memory;
mod session;
mod state;
mod value;

use eval::*;
use memory::*;
use state::*;
use value::*;

pub use session::SimulationSession;

#[cfg(test)]
mod tests;
