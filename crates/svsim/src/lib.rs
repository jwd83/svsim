pub mod compiler;
pub mod design;
pub mod diag;
pub mod frontend;
pub mod hir;
pub mod sim;

pub use compiler::Compiler;
pub use design::CompiledDesign;
pub use diag::{Diagnostic, Error, Result, SourceSpan};
pub use hir::{HirDesign, ModuleDeclStyle, ModuleInstanceSummary, ModuleSummary, SourceFile};
pub use sim::SimulationSession;
