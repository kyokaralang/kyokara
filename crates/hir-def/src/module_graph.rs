//! Module graph — multi-file project structure.
//!
//! Tracks which modules exist, their file IDs, item trees, and module scopes.
//! Convention-based file layout: `math.ky` → module `math`,
//! `math/utils.ky` → module `math.utils`.

use std::path::{Path, PathBuf};

use kyokara_span::FileId;
use kyokara_stdx::FxHashMap;

use crate::item_tree::ItemTree;
use crate::name::Name;
use crate::resolver::ModuleScope;

/// A module path like `["math", "utils"]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub Vec<Name>);

impl ModulePath {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn single(name: Name) -> Self {
        Self(vec![name])
    }

    pub fn last(&self) -> Option<Name> {
        self.0.last().copied()
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

/// Information about a single module in the project.
pub struct ModuleInfo {
    pub file_id: FileId,
    pub path: PathBuf,
    pub item_tree: ItemTree,
    pub scope: ModuleScope,
    pub source: String,
}

impl std::fmt::Debug for ModuleInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleInfo")
            .field("file_id", &self.file_id)
            .field("path", &self.path)
            .finish()
    }
}

/// The project-wide module graph.
#[derive(Debug, Default)]
pub struct ModuleGraph {
    modules: FxHashMap<ModulePath, ModuleInfo>,
    /// Which module is the entry point (contains `main`).
    pub entry: Option<ModulePath>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: ModulePath, info: ModuleInfo) {
        self.modules.insert(path, info);
    }

    pub fn get(&self, path: &ModulePath) -> Option<&ModuleInfo> {
        self.modules.get(path)
    }

    pub fn get_mut(&mut self, path: &ModulePath) -> Option<&mut ModuleInfo> {
        self.modules.get_mut(path)
    }

    /// Resolve an import from a module. Looks up by the single-segment
    /// import name (e.g., `import math` → look for module path `["math"]`).
    pub fn resolve_import(&self, target: &Name) -> Option<&ModuleInfo> {
        // Try single-segment module path first.
        for (mod_path, info) in &self.modules {
            if mod_path.last() == Some(*target) {
                return Some(info);
            }
        }
        None
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ModulePath, &ModuleInfo)> {
        self.modules.iter()
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

/// Discover `.ky` source files under a root directory and map them to module paths.
///
/// Convention:
/// - `<root>/math.ky` → `ModulePath(["math"])`
/// - `<root>/math/utils.ky` → `ModulePath(["math", "utils"])`
/// - The entry file (passed as `entry`) is mapped to `ModulePath::root()`
///
/// Returns `(ModulePath, PathBuf)` pairs for each discovered file.
pub fn discover_modules(
    _root: &Path,
    entry: &Path,
    interner: &mut kyokara_intern::Interner,
) -> Vec<(ModulePath, PathBuf)> {
    let mut result = Vec::new();

    // The entry file is the root module.
    if entry.exists() {
        result.push((ModulePath::root(), entry.to_path_buf()));
    }

    // Discover sibling .ky files and subdirectory .ky files relative to the entry's parent.
    let base_dir = entry.parent().unwrap_or(Path::new("."));

    if let Ok(entries) = std::fs::read_dir(base_dir) {
        for entry_result in entries {
            let Ok(dir_entry) = entry_result else {
                continue;
            };
            let path = dir_entry.path();

            // Skip the entry file itself.
            if path == entry {
                continue;
            }

            if path.extension().is_some_and(|ext| ext == "ky") {
                // Sibling file: math.ky → ["math"]
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let name = Name::new(interner, stem);
                    result.push((ModulePath::single(name), path));
                }
            } else if path.is_dir() {
                // Subdirectory: scan for .ky files
                discover_subdir(&path, &[], base_dir, interner, &mut result);
            }
        }
    }

    result
}

#[allow(clippy::only_used_in_recursion)]
fn discover_subdir(
    dir: &Path,
    prefix: &[Name],
    base_dir: &Path,
    interner: &mut kyokara_intern::Interner,
    result: &mut Vec<(ModulePath, PathBuf)>,
) {
    let dir_name = match dir.file_name().and_then(|s| s.to_str()) {
        Some(n) => Name::new(interner, n),
        None => return,
    };

    let mut segments = prefix.to_vec();
    segments.push(dir_name);

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry_result in entries {
            let Ok(dir_entry) = entry_result else {
                continue;
            };
            let path = dir_entry.path();

            if path.extension().is_some_and(|ext| ext == "ky") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let name = Name::new(interner, stem);
                    let mut mod_segments = segments.clone();
                    mod_segments.push(name);
                    result.push((ModulePath(mod_segments), path));
                }
            } else if path.is_dir() {
                discover_subdir(&path, &segments, base_dir, interner, result);
            }
        }
    }
}
