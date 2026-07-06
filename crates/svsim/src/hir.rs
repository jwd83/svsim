use std::path::PathBuf;

use serde::Serialize;

use crate::diag::{Diagnostic, SourceSpan};
use crate::logic_value::LogicBits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PackedRange {
    pub msb: usize,
    pub lsb: usize,
}

impl PackedRange {
    pub fn width(&self) -> usize {
        if self.msb >= self.lsb {
            self.msb - self.lsb + 1
        } else {
            self.lsb - self.msb + 1
        }
    }

    pub fn low(&self) -> usize {
        self.msb.min(self.lsb)
    }

    pub fn high(&self) -> usize {
        self.msb.max(self.lsb)
    }

    pub fn contains_index(&self, index: usize) -> bool {
        (self.low()..=self.high()).contains(&index)
    }

    pub fn index_offset(&self, index: usize) -> Option<usize> {
        self.contains_index(index).then_some(index - self.low())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PortDirection {
    Input,
    Output,
    Inout,
    Ref,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NetKind {
    Supply0,
    Supply1,
    Tri,
    Triand,
    Trior,
    Trireg,
    Tri0,
    Tri1,
    Uwire,
    Wire,
    Wand,
    Wor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StorageKind {
    Variable,
    Net(NetKind),
}

impl StorageKind {
    pub fn is_variable(self) -> bool {
        matches!(self, Self::Variable)
    }

    pub fn is_net(self) -> bool {
        matches!(self, Self::Net(_))
    }

    pub fn net_kind(self) -> Option<NetKind> {
        match self {
            Self::Variable => None,
            Self::Net(kind) => Some(kind),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortDecl {
    pub name: String,
    pub direction: PortDirection,
    pub storage: StorageKind,
    pub range: Option<PackedRange>,
    pub span: Option<SourceSpan>,
}

impl PortDecl {
    pub fn width(&self) -> usize {
        self.range.map_or(1, |range| range.width())
    }

    pub fn is_variable(&self) -> bool {
        self.storage.is_variable()
    }

    pub fn is_net(&self) -> bool {
        self.storage.is_net()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignalDecl {
    pub name: String,
    pub storage: StorageKind,
    pub range: Option<PackedRange>,
    pub span: Option<SourceSpan>,
}

impl SignalDecl {
    pub fn width(&self) -> usize {
        self.range.map_or(1, |range| range.width())
    }

    pub fn is_variable(&self) -> bool {
        self.storage.is_variable()
    }

    pub fn is_net(&self) -> bool {
        self.storage.is_net()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParameterDecl {
    pub name: String,
    pub range: Option<PackedRange>,
    pub default_value: Expr,
    pub span: Option<SourceSpan>,
}

impl ParameterDecl {
    pub fn width(&self) -> usize {
        self.range.map_or(32, |range| range.width())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryDecl {
    pub name: String,
    pub storage: StorageKind,
    pub element_range: Option<PackedRange>,
    pub index_range: PackedRange,
    pub span: Option<SourceSpan>,
}

impl MemoryDecl {
    pub fn element_width(&self) -> usize {
        self.element_range.map_or(1, |range| range.width())
    }

    pub fn depth(&self) -> usize {
        self.index_range.width()
    }

    pub fn is_variable(&self) -> bool {
        self.storage.is_variable()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NumericLiteral {
    pub bits: LogicBits,
    pub width: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum UnaryOp {
    BitNot,
    Negate,
    LogicalNot,
    ReductionAnd,
    ReductionNand,
    ReductionOr,
    ReductionXor,
    Signed,
    Unsigned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BinaryOp {
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    ArithmeticShiftRight,
    LogicalAnd,
    LogicalOr,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Add,
    Sub,
    Mul,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Expr {
    Ident(String),
    Literal(NumericLiteral),
    Concat(Vec<Expr>),
    Repeat {
        count: usize,
        expr: Box<Expr>,
    },
    MemoryRead {
        memory: String,
        index: Box<Expr>,
    },
    BitSelect {
        expr: Box<Expr>,
        index: usize,
    },
    PartSelect {
        expr: Box<Expr>,
        msb: usize,
        lsb: usize,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        when_true: Box<Expr>,
        when_false: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum LValue {
    Signal(String),
    Concat(Vec<LValue>),
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
        index: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum AssignmentKind {
    Blocking,
    Nonblocking,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ProcBlockKind {
    AlwaysComb,
    AlwaysFf {
        clock: String,
        async_reset: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseStmtItem {
    pub patterns: Vec<Expr>,
    pub body: Stmt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Stmt {
    Empty,
    Block(Vec<Stmt>),
    Assign {
        kind: AssignmentKind,
        target: LValue,
        expr: Expr,
    },
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    Case {
        expr: Expr,
        items: Vec<CaseStmtItem>,
        default: Option<Box<Stmt>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcBlock {
    pub kind: ProcBlockKind,
    pub body: Stmt,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContinuousAssign {
    pub target: LValue,
    pub expr: Expr,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NamedPortConnection {
    pub port_name: String,
    pub expr: Expr,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NamedParameterAssign {
    pub parameter_name: String,
    pub expr: Expr,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleInstanceSummary {
    pub module_name: String,
    pub instance_name: String,
    pub span: Option<SourceSpan>,
    pub parameter_overrides: Vec<NamedParameterAssign>,
    pub connections: Vec<NamedPortConnection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ModuleDeclStyle {
    Ansi,
    NonAnsi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleSummary {
    pub name: String,
    pub style: ModuleDeclStyle,
    pub span: Option<SourceSpan>,
    pub ports: Vec<PortDecl>,
    pub parameters: Vec<ParameterDecl>,
    pub signals: Vec<SignalDecl>,
    pub memories: Vec<MemoryDecl>,
    pub continuous_assignments: Vec<ContinuousAssign>,
    pub proc_blocks: Vec<ProcBlock>,
    pub instantiations: Vec<ModuleInstanceSummary>,
    pub unsupported: Vec<Diagnostic>,
    /// Parameters whose lowering-time default value is baked ("frozen") into
    /// this module's HIR, mapped to a description of the first construct that
    /// consumed them. Lowering runs once per module, before instantiation, so
    /// declaration ranges, constant selects, replication counts, unrolled
    /// `for` loops, pruned `if` branches, and generate conditions are all
    /// evaluated with parameter *defaults*. Elaboration rejects instance
    /// overrides that would change a frozen parameter's value, because the
    /// already-lowered HIR could not reflect them.
    #[serde(skip)]
    pub frozen_parameters: std::collections::BTreeMap<String, String>,
}

impl ModuleSummary {
    pub fn port(&self, name: &str) -> Option<&PortDecl> {
        self.ports.iter().find(|port| port.name == name)
    }

    pub fn signal_decl(&self, name: &str) -> Option<&SignalDecl> {
        self.signals.iter().find(|signal| signal.name == name)
    }

    pub fn memory_decl(&self, name: &str) -> Option<&MemoryDecl> {
        self.memories.iter().find(|memory| memory.name == name)
    }

    pub fn parameter_decl(&self, name: &str) -> Option<&ParameterDecl> {
        self.parameters.iter().find(|param| param.name == name)
    }

    pub fn signal_width(&self, name: &str) -> Option<usize> {
        self.port(name)
            .map(PortDecl::width)
            .or_else(|| self.signal_decl(name).map(SignalDecl::width))
            .or_else(|| self.parameter_decl(name).map(ParameterDecl::width))
    }
}

pub(crate) fn expr_to_lvalue(expr: &Expr) -> Option<LValue> {
    match expr {
        Expr::Ident(name) => Some(LValue::Signal(name.clone())),
        Expr::Concat(exprs) => {
            let mut items = Vec::with_capacity(exprs.len());
            for expr in exprs {
                items.push(expr_to_lvalue(expr)?);
            }
            Some(LValue::Concat(items))
        }
        Expr::BitSelect { expr, index } => match expr.as_ref() {
            Expr::Ident(signal) => Some(LValue::BitSelect {
                signal: signal.clone(),
                index: *index,
            }),
            _ => None,
        },
        Expr::PartSelect { expr, msb, lsb } => match expr.as_ref() {
            Expr::Ident(signal) => Some(LValue::PartSelect {
                signal: signal.clone(),
                msb: *msb,
                lsb: *lsb,
            }),
            _ => None,
        },
        Expr::MemoryRead { memory, index } => Some(LValue::MemoryElement {
            memory: memory.clone(),
            index: index.clone(),
        }),
        Expr::Literal(_)
        | Expr::Repeat { .. }
        | Expr::Unary { .. }
        | Expr::Binary { .. }
        | Expr::Ternary { .. } => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub modules: Vec<ModuleSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct HirDesign {
    files: Vec<SourceFile>,
}

impl HirDesign {
    pub fn new(files: Vec<SourceFile>) -> Self {
        Self { files }
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn module_count(&self) -> usize {
        self.files.iter().map(|file| file.modules.len()).sum()
    }

    pub fn module_names(&self) -> Vec<&str> {
        self.files
            .iter()
            .flat_map(|file| file.modules.iter().map(|module| module.name.as_str()))
            .collect()
    }

    pub fn module(&self, name: &str) -> Option<&ModuleSummary> {
        self.files
            .iter()
            .flat_map(|file| file.modules.iter())
            .find(|module| module.name == name)
    }

    pub fn module_source_path(&self, name: &str) -> Option<&PathBuf> {
        self.files.iter().find_map(|file| {
            file.modules
                .iter()
                .any(|module| module.name == name)
                .then_some(&file.path)
        })
    }
}
