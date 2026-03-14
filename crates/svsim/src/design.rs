use std::path::Path;
use std::path::PathBuf;

use crate::diag::Error;
use crate::diag::Result;
use crate::hir::{HirDesign, SourceFile};
use crate::sim::SimulationSession;
use crate::test::{JsonTestReport, JsonTestSuite};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignHierarchy {
    pub top_module: String,
    pub children: Vec<InstanceHierarchy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstanceHierarchy {
    pub instance_name: String,
    pub module_name: String,
    pub children: Vec<InstanceHierarchy>,
}

#[derive(Debug, Clone)]
pub struct CompiledDesign {
    hir: HirDesign,
    search_paths: Vec<PathBuf>,
    top_module: String,
}

impl CompiledDesign {
    pub(crate) fn new(
        search_paths: Vec<PathBuf>,
        files: Vec<SourceFile>,
        top_module: String,
    ) -> Self {
        let hir = HirDesign::new(files);

        Self {
            hir,
            search_paths,
            top_module,
        }
    }

    pub fn hir(&self) -> &HirDesign {
        &self.hir
    }

    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    pub fn top_module(&self) -> Option<&str> {
        Some(self.top_module.as_str())
    }

    pub fn hierarchy(&self) -> Result<DesignHierarchy> {
        Ok(DesignHierarchy {
            top_module: self.top_module.clone(),
            children: build_hierarchy(&self.hir, &self.top_module, &mut Vec::new())?,
        })
    }

    pub fn instantiate_top(&self) -> Result<SimulationSession> {
        SimulationSession::new(self.clone())
    }

    pub fn run_json_file(&self, path: impl AsRef<Path>) -> Result<JsonTestReport> {
        JsonTestSuite::load_file(path)?.run(self)
    }
}

fn build_hierarchy(
    hir: &HirDesign,
    module_name: &str,
    stack: &mut Vec<String>,
) -> Result<Vec<InstanceHierarchy>> {
    if stack.iter().any(|name| name == module_name) {
        return Err(Error::Unsupported(format!(
            "recursive instantiation detected at {} -> {}",
            stack.join(" -> "),
            module_name
        )));
    }

    let module = hir
        .module(module_name)
        .ok_or_else(|| Error::Resolve(format!("module '{}' was not compiled", module_name)))?;
    stack.push(module_name.to_owned());

    let mut children = Vec::with_capacity(module.instantiations.len());
    for instance in &module.instantiations {
        children.push(InstanceHierarchy {
            instance_name: instance.instance_name.clone(),
            module_name: instance.module_name.clone(),
            children: build_hierarchy(hir, &instance.module_name, stack)?,
        });
    }

    stack.pop();
    Ok(children)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{CompiledDesign, DesignHierarchy};
    use crate::Compiler;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn hierarchy_reports_leaf_top_module() {
        let repo = repo_root();
        let design: CompiledDesign = Compiler::new()
            .compile_file(repo.join("parts/basic/nand_gate.sv"))
            .expect("compile nand_gate");

        assert_eq!(
            design.hierarchy().expect("hierarchy"),
            DesignHierarchy {
                top_module: "nand_gate".into(),
                children: Vec::new(),
            }
        );
    }

    #[test]
    fn hierarchy_reports_nested_instance_tree() {
        let repo = repo_root();
        let design: CompiledDesign = Compiler::new()
            .compile_file(repo.join("parts/basic/full_adder.sv"))
            .expect("compile full_adder");
        let hierarchy = design.hierarchy().expect("hierarchy");

        assert_eq!(hierarchy.top_module, "full_adder");
        assert_eq!(
            hierarchy
                .children
                .iter()
                .map(|child| (child.instance_name.as_str(), child.module_name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("u_half1", "half_adder"),
                ("u_half2", "half_adder"),
                ("u_or_carry", "or_gate"),
            ]
        );

        let first_half = &hierarchy.children[0];
        assert_eq!(
            first_half
                .children
                .iter()
                .map(|child| (child.instance_name.as_str(), child.module_name.as_str()))
                .collect::<Vec<_>>(),
            vec![("u_xor", "xor_gate"), ("u_and", "and_gate")]
        );

        let xor_gate = &first_half.children[0];
        assert_eq!(
            xor_gate
                .children
                .iter()
                .map(|child| child.module_name.as_str())
                .collect::<Vec<_>>(),
            vec!["nand_gate", "nand_gate", "nand_gate", "nand_gate"]
        );
    }
}
