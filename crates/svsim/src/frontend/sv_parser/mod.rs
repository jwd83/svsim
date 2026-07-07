//! `sv-parser` integration: parses SystemVerilog and lowers the syntax tree
//! into owned HIR. `sv_parser` crate types stop at this module boundary.
//!
//! Split by responsibility: `module_structure` (modules, ports, parameters,
//! declarations, generate, instantiation), `statements` (procedural
//! statements), `expressions` (expressions, lvalues, selects), `literals`
//! (numeric/string literals), `const_eval` (lowering-time constant
//! evaluation and frozen-parameter recording), and `loop_unroll` (for-loop
//! unrolling — elaboration work done at lowering time). This file keeps the
//! public `SvParserFrontend` entry points and shared span/identifier
//! plumbing.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use sv_parser::{
    AlwaysConstruct, AlwaysKeyword, AnsiPortDeclaration, BinaryOperator, CaseItem as SvCaseItem,
    CaseStatement, CondPredicate, ConditionalStatement, ConstantExpression,
    ConstantPartSelectRange, ConstantRange, ConstantSelect, ContinuousAssign, DataDeclaration,
    DataType, DataTypeOrImplicit, Define, Defines, Expression, FunctionSubroutineCall,
    HierarchicalIdentifier, ImplicitDataType, InitialConstruct, Keyword,
    ListOfParameterAssignments, ListOfPortConnections, LocalParameterDeclaration, Locate,
    ModuleDeclarationAnsi, ModuleDeclarationNonansi, ModuleInstantiation, ModuleOrGenerateItem,
    ModuleOrGenerateItemDeclaration, NamedPortConnection, NetDeclaration, NetLvalue,
    NonPortModuleItem, PackageOrGenerateItemDeclaration, ParameterDeclaration,
    ParameterPortDeclaration, ParameterPortList, Paren, PartSelectRange, PortDirection, Primary,
    PsOrHierarchicalNetIdentifier, RefNode, Select, SeqBlock, Statement, StatementItem,
    StatementOrNull, SyntaxTree, UnaryOperator, UnpackedDimension, VariableAssignment,
    VariableDeclAssignment, VariableDimension, VariableLvalue, VariablePortType, parse_sv,
    parse_sv_str, unwrap_node,
};

use crate::bit_value::BitValue;
use crate::diag::{Diagnostic, Error, Result, SourceSpan};
use crate::expr_eval::{Value, eval_expr, resolve_parameter_defaults};
use crate::hir::{
    AssignmentKind, BinaryOp, CaseStmtItem, ContinuousAssign as HirContinuousAssign, Expr, LValue,
    MemoryDecl, ModuleDeclStyle, ModuleInstanceSummary, ModuleSummary,
    NamedParameterAssign as HirNamedParameterAssign, NamedPortConnection as HirNamedPortConnection,
    NetKind, NumericLiteral, PackedRange, ParameterDecl, PortDecl,
    PortDirection as HirPortDirection, ProcBlock, ProcBlockKind, SignalDecl, SourceFile, Stmt,
    StorageKind, UnaryOp,
};
use crate::logic_value::{LogicBit, LogicBits};

type LowerResult<T> = std::result::Result<T, Diagnostic>;
const PROCEDURAL_FOR_UNROLL_LIMIT: usize = 16_384;

#[derive(Debug, Clone, Default)]
pub struct SvParserFrontend {
    include_paths: Vec<PathBuf>,
}

impl SvParserFrontend {
    pub fn new(include_paths: Vec<PathBuf>) -> Self {
        Self { include_paths }
    }

    pub fn parse_file(&self, path: &Path) -> Result<SourceFile> {
        let defines: Defines = HashMap::<String, Option<Define>>::new();
        let include_paths = self.include_paths_for(path);
        let (syntax_tree, _) =
            parse_sv(path, &defines, &include_paths, false, false).map_err(|error| {
                Error::Parse(format!("failed to parse {}: {error}", path.display()))
            })?;

        lower_source_file(&syntax_tree, path)
    }

    pub fn parse_str(&self, virtual_path: impl AsRef<Path>, source: &str) -> Result<SourceFile> {
        let path = virtual_path.as_ref();
        let defines: Defines = HashMap::<String, Option<Define>>::new();
        let include_paths = self.include_paths_for(path);
        let (syntax_tree, _) = parse_sv_str(source, path, &defines, &include_paths, false, false)
            .map_err(|error| {
            Error::Parse(format!("failed to parse {}: {error}", path.display()))
        })?;

        lower_source_file(&syntax_tree, path)
    }

    fn include_paths_for(&self, path: &Path) -> Vec<PathBuf> {
        let mut include_paths = Vec::with_capacity(self.include_paths.len() + 1);
        if let Some(parent) = path.parent() {
            include_paths.push(parent.to_path_buf());
        }
        include_paths.extend(self.include_paths.iter().cloned());
        include_paths
    }
}

fn locate_usize(syntax_tree: &SyntaxTree, locate: &Locate) -> LowerResult<usize> {
    syntax_tree
        .get_str(locate)
        .ok_or_else(|| unsupported("failed to read integer text", None))?
        .replace('_', "")
        .parse::<usize>()
        .map_err(|_| unsupported("failed to parse integer", None))
}

fn span_from_locate(path: &Path, locate: Locate) -> SourceSpan {
    SourceSpan {
        path: path.to_path_buf(),
        line: locate.line as usize,
        column: 1,
    }
}

fn symbol_text(syntax_tree: &SyntaxTree, symbol: &sv_parser::Symbol) -> LowerResult<String> {
    syntax_tree
        .get_str(&symbol.nodes.0)
        .map(str::to_owned)
        .ok_or_else(|| unsupported("failed to read symbol text", None))
}

fn unsupported(message: impl Into<String>, span: Option<SourceSpan>) -> Diagnostic {
    Diagnostic {
        message: message.into(),
        span,
    }
}

/// Attaches a span to a diagnostic that lacks one, so leaf helpers without
/// source context (literal parsing, operator tables) get located at their
/// call sites.
fn with_fallback_span(diagnostic: Diagnostic, span: Option<SourceSpan>) -> Diagnostic {
    if diagnostic.span.is_some() {
        return diagnostic;
    }
    Diagnostic { span, ..diagnostic }
}

/// Best-effort source span for any syntax node: the first `Locate` leaf in
/// preorder. Lowering functions compute this once per construct so their
/// `unsupported` diagnostics point at the offending source line.
fn span_of_node<'a>(path: &Path, node: impl Into<RefNode<'a>>) -> Option<SourceSpan> {
    for sub in node.into() {
        if let RefNode::Locate(locate) = sub {
            return Some(span_from_locate(path, *locate));
        }
    }
    None
}

fn identifier_name_from_node(
    syntax_tree: &SyntaxTree,
    node: RefNode<'_>,
) -> Option<(String, Locate)> {
    let locate = get_identifier(node)?;
    let name = syntax_tree.get_str(&locate)?.to_owned();
    Some((name, locate))
}

fn get_identifier(node: RefNode) -> Option<Locate> {
    match unwrap_node!(node, SimpleIdentifier, EscapedIdentifier) {
        Some(RefNode::SimpleIdentifier(identifier)) => Some(identifier.nodes.0),
        Some(RefNode::EscapedIdentifier(identifier)) => Some(identifier.nodes.0),
        _ => None,
    }
}

mod literals;
use literals::*;

mod const_eval;
use const_eval::*;

mod loop_unroll;
use loop_unroll::*;

mod expressions;
use expressions::*;

mod statements;
use statements::*;

mod module_structure;
use module_structure::*;

#[cfg(test)]
mod tests;
