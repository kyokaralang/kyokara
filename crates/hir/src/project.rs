use std::path::{Path, PathBuf};

use kyokara_hir_def::module_graph::{OwnedModulePath, discover_module_files};

const PACKAGE_MANIFEST: &str = "kyokara.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiscoveryDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

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
    pub discovery_diagnostics: Vec<ProjectDiscoveryDiagnostic>,
}

impl ProjectLoadPlan {
    pub fn from_entry_file(entry_file: &Path) -> Self {
        match detect_package_layout(entry_file) {
            Ok(Some(layout)) => {
                let modules = discover_module_files(&layout.source_root, entry_file);
                Self {
                    entry_package: 0,
                    packages: vec![PackageLoadPlan {
                        package_root: Some(layout.package_root),
                        source_root: layout.source_root,
                        entry_file: entry_file.to_path_buf(),
                        modules,
                    }],
                    discovery_diagnostics: Vec::new(),
                }
            }
            Ok(None) => Self::legacy_single_package(entry_file, Vec::new()),
            Err(diag) => Self::legacy_single_package(entry_file, vec![diag]),
        }
    }

    fn legacy_single_package(
        entry_file: &Path,
        discovery_diagnostics: Vec<ProjectDiscoveryDiagnostic>,
    ) -> Self {
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
            discovery_diagnostics,
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

pub fn has_package_manifest_candidate(entry_file: &Path) -> bool {
    package_manifest_path(entry_file).is_some_and(|path| path.is_file())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageKind {
    Bin,
    Lib,
}

impl PackageKind {
    fn from_entry_file(entry_file: &Path) -> Option<Self> {
        match entry_file.file_name().and_then(|name| name.to_str()) {
            Some("main.ky") => Some(Self::Bin),
            Some("lib.ky") => Some(Self::Lib),
            _ => None,
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "bin" => Some(Self::Bin),
            "lib" => Some(Self::Lib),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Lib => "lib",
        }
    }

    fn entry_file_name(self) -> &'static str {
        match self {
            Self::Bin => "main.ky",
            Self::Lib => "lib.ky",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetectedPackageLayout {
    package_root: PathBuf,
    source_root: PathBuf,
}

fn detect_package_layout(
    entry_file: &Path,
) -> Result<Option<DetectedPackageLayout>, ProjectDiscoveryDiagnostic> {
    let Some(manifest_path) = package_manifest_path(entry_file) else {
        return Ok(None);
    };
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest_source = std::fs::read_to_string(&manifest_path).map_err(|err| {
        invalid_manifest(&manifest_path, format!("failed to read manifest: {err}"))
    })?;
    let expected_kind = PackageKind::from_entry_file(entry_file)
        .expect("package manifest paths are only built for main.ky/lib.ky entries");
    let manifest_kind = parse_package_kind(&manifest_path, &manifest_source)?;

    if manifest_kind != expected_kind {
        return Err(invalid_manifest(
            &manifest_path,
            format!(
                "package kind `{}` expects entry file `{}`",
                manifest_kind.as_str(),
                manifest_kind.entry_file_name()
            ),
        ));
    }

    let package_root = manifest_path
        .parent()
        .expect("manifest file should have a parent directory")
        .to_path_buf();
    Ok(Some(DetectedPackageLayout {
        source_root: package_root.join("src"),
        package_root,
    }))
}

fn package_manifest_path(entry_file: &Path) -> Option<PathBuf> {
    let expected_kind = PackageKind::from_entry_file(entry_file)?;
    let source_root = entry_file.parent()?;
    if source_root.file_name().and_then(|name| name.to_str()) != Some("src") {
        return None;
    }
    if entry_file.file_name().and_then(|name| name.to_str())
        != Some(expected_kind.entry_file_name())
    {
        return None;
    }
    Some(source_root.parent()?.join(PACKAGE_MANIFEST))
}

fn parse_package_kind(
    manifest_path: &Path,
    manifest_source: &str,
) -> Result<PackageKind, ProjectDiscoveryDiagnostic> {
    let manifest = manifest_source
        .parse::<toml::Value>()
        .map_err(|err| invalid_manifest(manifest_path, format!("failed to parse TOML: {err}")))?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid_manifest(manifest_path, "missing [package] table"))?;
    let _name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid_manifest(manifest_path, "package.name must be a non-empty string")
        })?;
    let _edition = package
        .get("edition")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid_manifest(manifest_path, "package.edition must be a non-empty string")
        })?;
    let kind = package
        .get("kind")
        .and_then(toml::Value::as_str)
        .and_then(PackageKind::parse)
        .ok_or_else(|| invalid_manifest(manifest_path, "package.kind must be `bin` or `lib`"))?;
    Ok(kind)
}

fn invalid_manifest(manifest_path: &Path, detail: impl Into<String>) -> ProjectDiscoveryDiagnostic {
    ProjectDiscoveryDiagnostic {
        path: manifest_path.to_path_buf(),
        message: format!(
            "invalid package manifest `{}`: {}",
            manifest_path.display(),
            detail.into()
        ),
    }
}
