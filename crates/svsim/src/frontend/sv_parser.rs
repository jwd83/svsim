use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sv_parser::{Define, Defines, Locate, RefNode, SyntaxTree, parse_sv, unwrap_node};

use crate::diag::{Error, Result};
use crate::hir::{ModuleDeclStyle, ModuleInstanceSummary, ModuleSummary, SourceFile};

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
        let (syntax_tree, _) = parse_sv(path, &defines, &self.include_paths, false, false)
            .map_err(|error| {
                Error::Parse(format!("failed to parse {}: {error}", path.display()))
            })?;
        let mut modules = collect_modules(&syntax_tree, path);
        let instantiations = collect_module_instantiations(&syntax_tree, path);

        for instantiation in instantiations {
            if let Some(module) =
                find_enclosing_module_mut(&mut modules, instantiation.span.as_ref())
            {
                module.instantiations.push(instantiation);
            }
        }

        Ok(SourceFile {
            path: path.to_path_buf(),
            modules,
        })
    }
}

fn collect_modules(syntax_tree: &SyntaxTree, path: &Path) -> Vec<ModuleSummary> {
    let mut modules = Vec::new();

    for node in syntax_tree {
        match node {
            RefNode::ModuleDeclarationAnsi(decl) => {
                if let Some((name, locate)) =
                    module_name_from_node(syntax_tree, RefNode::from(decl))
                {
                    modules.push(ModuleSummary {
                        name,
                        style: ModuleDeclStyle::Ansi,
                        span: Some(crate::diag::SourceSpan {
                            path: path.to_path_buf(),
                            line: locate.line as usize,
                            column: 1,
                        }),
                        instantiations: Vec::new(),
                    });
                }
            }
            RefNode::ModuleDeclarationNonansi(decl) => {
                if let Some((name, locate)) =
                    module_name_from_node(syntax_tree, RefNode::from(decl))
                {
                    modules.push(ModuleSummary {
                        name,
                        style: ModuleDeclStyle::NonAnsi,
                        span: Some(crate::diag::SourceSpan {
                            path: path.to_path_buf(),
                            line: locate.line as usize,
                            column: 1,
                        }),
                        instantiations: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }

    modules
}

fn collect_module_instantiations(
    syntax_tree: &SyntaxTree,
    path: &Path,
) -> Vec<ModuleInstanceSummary> {
    let mut instantiations = Vec::new();

    for node in syntax_tree {
        if let RefNode::ModuleInstantiation(instantiation) = node {
            let Some((module_name, _)) =
                identifier_name_from_node(syntax_tree, RefNode::from(&instantiation.nodes.0))
            else {
                continue;
            };

            for hierarchical_instance in instantiation.nodes.2.contents() {
                let Some((instance_name, locate)) = identifier_name_from_node(
                    syntax_tree,
                    RefNode::from(&hierarchical_instance.nodes.0.nodes.0),
                ) else {
                    continue;
                };

                instantiations.push(ModuleInstanceSummary {
                    module_name: module_name.clone(),
                    instance_name,
                    span: Some(crate::diag::SourceSpan {
                        path: path.to_path_buf(),
                        line: locate.line as usize,
                        column: 1,
                    }),
                });
            }
        }
    }

    instantiations
}

fn find_enclosing_module_mut<'a>(
    modules: &'a mut [ModuleSummary],
    span: Option<&crate::diag::SourceSpan>,
) -> Option<&'a mut ModuleSummary> {
    let line = span?.line;
    let mut selected_index = None;

    for (index, module) in modules.iter().enumerate() {
        let Some(module_span) = module.span.as_ref() else {
            continue;
        };
        if module_span.line <= line {
            selected_index = Some(index);
        }
    }

    selected_index.map(|index| &mut modules[index])
}

fn module_name_from_node(syntax_tree: &SyntaxTree, node: RefNode<'_>) -> Option<(String, Locate)> {
    let identifier = unwrap_node!(node, ModuleIdentifier)?;
    identifier_name_from_node(syntax_tree, identifier)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SvParserFrontend;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn parse_file_collects_module_name() {
        let repo = repo_root();
        let frontend = SvParserFrontend::default();
        let source = frontend
            .parse_file(&repo.join("parts/basic/full_adder.sv"))
            .expect("parse full_adder");

        assert_eq!(source.modules.len(), 1);
        assert_eq!(source.modules[0].name, "full_adder");
        assert_eq!(source.modules[0].instantiations.len(), 3);
        assert_eq!(
            source.modules[0].instantiations[0].module_name,
            "half_adder"
        );
        assert_eq!(source.modules[0].instantiations[0].instance_name, "u_half1");
    }
}
