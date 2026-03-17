use std::path::PathBuf;

use serde::Serialize;

use crate::diag::{Diagnostic, SourceSpan};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortDecl {
    pub name: String,
    pub direction: PortDirection,
    pub range: Option<PackedRange>,
    pub span: Option<SourceSpan>,
}

impl PortDecl {
    pub fn width(&self) -> usize {
        self.range.map_or(1, |range| range.width())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignalDecl {
    pub name: String,
    pub range: Option<PackedRange>,
    pub span: Option<SourceSpan>,
}

impl SignalDecl {
    pub fn width(&self) -> usize {
        self.range.map_or(1, |range| range.width())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryDecl {
    pub name: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NumericLiteral {
    pub bits: u64,
    pub width: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum UnaryOp {
    BitNot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BinaryOp {
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    LogicalAnd,
    LogicalOr,
    Eq,
    NotEq,
    Add,
    Sub,
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
    AlwaysFf { clock: String },
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
pub struct ModuleInstanceSummary {
    pub module_name: String,
    pub instance_name: String,
    pub span: Option<SourceSpan>,
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
    pub signals: Vec<SignalDecl>,
    pub memories: Vec<MemoryDecl>,
    pub continuous_assignments: Vec<ContinuousAssign>,
    pub proc_blocks: Vec<ProcBlock>,
    pub instantiations: Vec<ModuleInstanceSummary>,
    pub unsupported: Vec<Diagnostic>,
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

    pub fn signal_width(&self, name: &str) -> Option<usize> {
        self.port(name)
            .map(PortDecl::width)
            .or_else(|| self.signal_decl(name).map(SignalDecl::width))
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
