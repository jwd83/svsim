pub mod compiler;
pub mod design;
pub mod diag;
pub mod frontend;
pub mod hir;
pub mod sim;
pub mod test;

pub use compiler::Compiler;
pub use design::{CompiledDesign, DesignHierarchy, InstanceHierarchy};
pub use diag::{Diagnostic, Error, Result, SourceSpan};
pub use hir::{
    CaseStmtItem, HirDesign, MemoryDecl, ModuleDeclStyle, ModuleInstanceSummary, ModuleSummary,
    ProcBlock, ProcBlockKind, SourceFile, Stmt,
};
pub use sim::SimulationSession;
pub use test::{
    JsonTestCaseReport, JsonTestCorpusReport, JsonTestDirectoryReport, JsonTestDirectoryRunReport,
    JsonTestFailure, JsonTestReport, JsonTestSuite, JsonTestSuiteRunReport,
};
