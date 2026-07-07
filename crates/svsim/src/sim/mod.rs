//! Structural runtime for compiled designs: combinational settle,
//! sequential stepping, memory loading, and signal inspection.
//!
//! Split by responsibility: `value` (four-state values and primitive ops),
//! `eval` (expression/lvalue evaluation and driver staging), `state`
//! (hierarchical module state and bindings), `memory` (memory files and the
//! legacy ROM shim), `session` (public API and scheduler).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::bit_value::{BitValue, ParseBitValueError};
use crate::design::CompiledDesign;
use crate::diag::{Error, Result};
use crate::elaborate::{ElaboratedInstance, RuntimeObjectShape};
use crate::expr_eval::{LogicTruth, MemoryState, Value, ValueReader, eval_expr, values_case_equal};
use crate::fast_hash::{FxHashMap, FxHashSet};
use crate::hir::{
    AssignmentKind, Expr, HirDesign, LValue, ModuleSummary, PortDirection, ProcBlockKind, Stmt,
    StorageKind,
};
use crate::logic_ops::{logic_replace_slice, logic_slice, logic_value_from_bit};
use crate::logic_value::{LogicBit, LogicBits, LogicValue};
use crate::net_resolve::{DriveStrengthPair, NetDriver, resolve_net};

mod eval;
pub(crate) mod legacy_rom;
mod memory;
mod session;
mod state;
mod value;

use eval::*;
use legacy_rom::*;
use memory::*;
use state::*;
use value::*;

pub use session::SimulationSession;

#[cfg(test)]
mod tests;
