use std::io;
use std::path::{Path, PathBuf};

use kyokara_hir_def::module_graph::{OwnedModulePath, discover_module_files};

const PACKAGE_MANIFEST: &str = "kyokara.toml";
const PACKAGE_LOCKFILE: &str = "kyokara.lock";
const LOCKFILE_VERSION: i64 = 1;

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
    pub module_prefix: OwnedModulePath,
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
                let mut discovery_diagnostics =
                    reserved_deps_namespace_diagnostics(&layout.modules);
                let mut packages = vec![PackageLoadPlan {
                    package_root: Some(layout.package_root.clone()),
                    source_root: layout.source_root.clone(),
                    entry_file: entry_file.to_path_buf(),
                    module_prefix: OwnedModulePath::root(),
                    modules: layout.modules,
                }];

                for dependency in &layout.manifest.dependencies {
                    match discover_path_dependency_package(&layout.package_root, dependency) {
                        Ok(package) => {
                            discovery_diagnostics
                                .extend(reserved_deps_namespace_diagnostics(&package.modules));
                            packages.push(package);
                        }
                        Err(diag) => discovery_diagnostics.push(diag),
                    }
                }

                Self {
                    entry_package: 0,
                    packages,
                    discovery_diagnostics,
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
                module_prefix: OwnedModulePath::root(),
                modules,
            }],
            discovery_diagnostics,
        }
    }

    pub fn iter_modules(&self) -> impl Iterator<Item = (OwnedModulePath, &PathBuf)> {
        self.packages.iter().flat_map(|package| {
            package
                .modules
                .iter()
                .map(|(path, file)| (path.prefixed(&package.module_prefix), file))
        })
    }
}

pub fn discover_project_load_plan(entry_file: &Path) -> ProjectLoadPlan {
    ProjectLoadPlan::from_entry_file(entry_file)
}

pub fn has_package_manifest_candidate(entry_file: &Path) -> bool {
    package_manifest_path(entry_file).is_some_and(|path| path.is_file())
}

pub fn sync_package_lockfile_for_entry(entry_file: &Path) -> io::Result<Option<PathBuf>> {
    let Some(manifest_path) = package_manifest_path(entry_file) else {
        return Ok(None);
    };
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest_source = std::fs::read_to_string(&manifest_path)?;
    let Ok(manifest) = parse_package_manifest(&manifest_path, &manifest_source) else {
        return Ok(None);
    };

    let lockfile_path = manifest_path.with_file_name(PACKAGE_LOCKFILE);
    let lockfile = PackageLockfile {
        dependencies: manifest.dependencies,
    };
    let rendered = render_package_lockfile(&lockfile);
    let should_write = match std::fs::read_to_string(&lockfile_path) {
        Ok(existing) => existing != rendered,
        Err(err) if err.kind() == io::ErrorKind::NotFound => true,
        Err(err) => return Err(err),
    };
    if should_write {
        std::fs::write(&lockfile_path, rendered)?;
    }
    Ok(Some(lockfile_path))
}

pub fn package_entry_file_for_source(source_file: &Path) -> Option<PathBuf> {
    let source_root = source_file
        .ancestors()
        .find(|ancestor| ancestor.file_name().and_then(|name| name.to_str()) == Some("src"))?;
    let package_root = source_root.parent()?;
    let manifest_path = package_root.join(PACKAGE_MANIFEST);
    if !manifest_path.is_file() {
        return None;
    }

    let manifest_source = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest = parse_package_manifest(&manifest_path, &manifest_source).ok()?;
    let entry_file = source_root.join(manifest.kind.entry_file_name());
    entry_file.is_file().then_some(entry_file)
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
struct PathDependencySpec {
    alias: String,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageManifest {
    kind: PackageKind,
    dependencies: Vec<PathDependencySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageLockfile {
    dependencies: Vec<PathDependencySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetectedPackageLayout {
    package_root: PathBuf,
    source_root: PathBuf,
    modules: Vec<(OwnedModulePath, PathBuf)>,
    manifest: PackageManifest,
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
    let mut manifest = parse_package_manifest(&manifest_path, &manifest_source)?;
    manifest.dependencies = locked_dependencies_for_manifest(&manifest_path, &manifest);

    if manifest.kind != expected_kind {
        return Err(invalid_manifest(
            &manifest_path,
            format!(
                "package kind `{}` expects entry file `{}`",
                manifest.kind.as_str(),
                manifest.kind.entry_file_name()
            ),
        ));
    }

    let package_root = manifest_path
        .parent()
        .expect("manifest file should have a parent directory")
        .to_path_buf();
    let source_root = package_root.join("src");
    Ok(Some(DetectedPackageLayout {
        modules: discover_module_files(&source_root, entry_file),
        source_root,
        package_root,
        manifest,
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

fn parse_package_manifest(
    manifest_path: &Path,
    manifest_source: &str,
) -> Result<PackageManifest, ProjectDiscoveryDiagnostic> {
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
    let dependencies = parse_path_dependencies(manifest_path, &manifest)?;

    Ok(PackageManifest { kind, dependencies })
}

fn locked_dependencies_for_manifest(
    manifest_path: &Path,
    manifest: &PackageManifest,
) -> Vec<PathDependencySpec> {
    let lockfile_path = manifest_path.with_file_name(PACKAGE_LOCKFILE);
    if !lockfile_path.is_file() {
        return manifest.dependencies.clone();
    }

    let Ok(lockfile_source) = std::fs::read_to_string(&lockfile_path) else {
        return manifest.dependencies.clone();
    };
    let Ok(lockfile) = parse_package_lockfile(&lockfile_path, &lockfile_source) else {
        return manifest.dependencies.clone();
    };
    if !path_dependencies_match(&lockfile.dependencies, &manifest.dependencies) {
        return manifest.dependencies.clone();
    }

    lockfile.dependencies
}

fn parse_package_lockfile(
    lockfile_path: &Path,
    lockfile_source: &str,
) -> Result<PackageLockfile, ProjectDiscoveryDiagnostic> {
    let lockfile = lockfile_source
        .parse::<toml::Value>()
        .map_err(|err| invalid_manifest(lockfile_path, format!("failed to parse TOML: {err}")))?;
    let version = lockfile
        .get("version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| invalid_manifest(lockfile_path, "lockfile.version must be an integer"))?;
    if version != LOCKFILE_VERSION {
        return Err(invalid_manifest(
            lockfile_path,
            format!("lockfile.version must be {LOCKFILE_VERSION}"),
        ));
    }

    let dependencies = parse_path_dependencies(lockfile_path, &lockfile)?;
    Ok(PackageLockfile { dependencies })
}

fn render_package_lockfile(lockfile: &PackageLockfile) -> String {
    let mut dependencies = lockfile.dependencies.clone();
    dependencies.sort_by(|lhs, rhs| lhs.alias.cmp(&rhs.alias));

    let mut rendered = format!("version = {LOCKFILE_VERSION}\n\n[dependencies]\n");
    for dependency in dependencies {
        rendered.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            dependency.alias,
            dependency.path.display()
        ));
    }
    rendered
}

fn path_dependencies_match(lhs: &[PathDependencySpec], rhs: &[PathDependencySpec]) -> bool {
    let mut lhs_sorted = lhs.to_vec();
    let mut rhs_sorted = rhs.to_vec();
    lhs_sorted.sort_by(|a, b| a.alias.cmp(&b.alias));
    rhs_sorted.sort_by(|a, b| a.alias.cmp(&b.alias));
    lhs_sorted == rhs_sorted
}

fn parse_path_dependencies(
    manifest_path: &Path,
    manifest: &toml::Value,
) -> Result<Vec<PathDependencySpec>, ProjectDiscoveryDiagnostic> {
    let Some(dependencies) = manifest.get("dependencies") else {
        return Ok(Vec::new());
    };
    let deps_table = dependencies.as_table().ok_or_else(|| {
        invalid_manifest(
            manifest_path,
            "[dependencies] must be a TOML table of alias = { path = \"...\" } entries",
        )
    })?;

    let mut parsed = Vec::with_capacity(deps_table.len());
    for (alias, spec) in deps_table {
        if !is_identifier(alias) {
            return Err(invalid_manifest(
                manifest_path,
                format!("dependency alias `{alias}` must be a valid identifier"),
            ));
        }
        let spec_table = spec.as_table().ok_or_else(|| {
            invalid_manifest(
                manifest_path,
                format!("dependency `{alias}` must use inline table syntax"),
            )
        })?;
        let path = spec_table
            .get("path")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                invalid_manifest(
                    manifest_path,
                    format!("dependency `{alias}` must declare a non-empty path"),
                )
            })?;
        let unsupported_keys: Vec<_> = spec_table
            .keys()
            .filter(|key| key.as_str() != "path")
            .cloned()
            .collect();
        if !unsupported_keys.is_empty() {
            return Err(invalid_manifest(
                manifest_path,
                format!(
                    "dependency `{alias}` uses unsupported keys: {}",
                    unsupported_keys.join(", ")
                ),
            ));
        }
        parsed.push(PathDependencySpec {
            alias: alias.clone(),
            path: PathBuf::from(path),
        });
    }

    Ok(parsed)
}

fn discover_path_dependency_package(
    importing_package_root: &Path,
    dependency: &PathDependencySpec,
) -> Result<PackageLoadPlan, ProjectDiscoveryDiagnostic> {
    let package_root = importing_package_root.join(&dependency.path);
    let manifest_path = package_root.join(PACKAGE_MANIFEST);
    if !manifest_path.is_file() {
        return Err(ProjectDiscoveryDiagnostic {
            path: manifest_path,
            message: format!(
                "invalid package manifest `{}`: dependency `{}` is missing kyokara.toml",
                package_root.join(PACKAGE_MANIFEST).display(),
                dependency.alias
            ),
        });
    }

    let manifest_source = std::fs::read_to_string(&manifest_path).map_err(|err| {
        invalid_manifest(&manifest_path, format!("failed to read manifest: {err}"))
    })?;
    let manifest = parse_package_manifest(&manifest_path, &manifest_source)?;
    if manifest.kind != PackageKind::Lib {
        return Err(ProjectDiscoveryDiagnostic {
            path: manifest_path.clone(),
            message: format!(
                "invalid package manifest `{}`: dependencies must be lib packages",
                manifest_path.display()
            ),
        });
    }

    let source_root = package_root.join("src");
    let entry_file = source_root.join("lib.ky");
    if !entry_file.is_file() {
        return Err(ProjectDiscoveryDiagnostic {
            path: entry_file.clone(),
            message: format!(
                "invalid package manifest `{}`: dependency `{}` is missing src/lib.ky",
                manifest_path.display(),
                dependency.alias
            ),
        });
    }

    Ok(PackageLoadPlan {
        package_root: Some(package_root.clone()),
        source_root: source_root.clone(),
        entry_file: entry_file.clone(),
        module_prefix: OwnedModulePath(vec!["deps".to_owned(), dependency.alias.clone()]),
        modules: discover_module_files(&source_root, &entry_file),
    })
}

fn reserved_deps_namespace_diagnostics(
    modules: &[(OwnedModulePath, PathBuf)],
) -> Vec<ProjectDiscoveryDiagnostic> {
    modules
        .iter()
        .filter_map(|(path, file)| {
            (path.0.first().map(String::as_str) == Some("deps")).then(|| {
                ProjectDiscoveryDiagnostic {
                    path: file.clone(),
                    message: format!(
                        "module path `{}` is reserved for dependency imports",
                        if path.0.is_empty() {
                            "deps".to_string()
                        } else {
                            path.0.join(".")
                        }
                    ),
                }
            })
        })
        .collect()
}

fn is_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
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
