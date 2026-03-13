use std::path::{Path, PathBuf};

use kyokara_hir_def::module_graph::{OwnedModulePath, discover_module_files};

/// Discovery plan for a single package/source root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageLoadPlan {
    pub package_root: Option<PathBuf>,
    pub source_root: PathBuf,
    pub entry_file: PathBuf,
    pub modules: Vec<(OwnedModulePath, PathBuf)>,
}

/// Project/package discovery plan used by `check_project`.
///
/// Phase 0 keeps the current single-package behavior, but lifts project
/// discovery into an explicit abstraction so later package phases can extend
/// this plan without rewriting the checker/eval/codegen entrypoints again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLoadPlan {
    pub entry_package: usize,
    pub packages: Vec<PackageLoadPlan>,
}

impl ProjectLoadPlan {
    pub fn from_entry_file(entry_file: &Path) -> Self {
        let source_root = entry_file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let modules = discover_module_files(&source_root, entry_file);

        Self {
            entry_package: 0,
            packages: vec![PackageLoadPlan {
                package_root: None,
                source_root,
                entry_file: entry_file.to_path_buf(),
                modules,
            }],
        }
    }

    pub fn iter_modules(&self) -> impl Iterator<Item = (&OwnedModulePath, &PathBuf)> {
        self.packages
            .iter()
            .flat_map(|package| package.modules.iter().map(|(path, file)| (path, file)))
    }
}

pub fn discover_project_load_plan(entry_file: &Path) -> ProjectLoadPlan {
    ProjectLoadPlan::from_entry_file(entry_file)
}
