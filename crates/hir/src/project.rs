use std::collections::{HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use kyokara_hir_def::module_graph::{OwnedModulePath, discover_module_files};
use semver::{Version, VersionReq};

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
                let mut discovery_diagnostics = Vec::new();
                let mut packages = Vec::new();
                let mut active_package_roots = HashSet::new();
                collect_dependency_packages(
                    layout,
                    OwnedModulePath::root(),
                    &mut packages,
                    &mut discovery_diagnostics,
                    &mut active_package_roots,
                );

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
    let manifest = parse_package_manifest(&manifest_path, &manifest_source)
        .map_err(|diag| io::Error::other(diag.message))?;
    let package_root = normalize_existing_path(
        manifest_path
            .parent()
            .expect("manifest file should have a parent directory"),
    );
    let dependencies = resolve_dependencies_for_manifest(&manifest_path, &manifest, &package_root)
        .map_err(|diag| io::Error::other(diag.message))?;
    validate_dependency_graph(&package_root, &package_root, &dependencies)
        .map_err(|diag| io::Error::other(diag.message))?;

    let lockfile_path = manifest_path.with_file_name(PACKAGE_LOCKFILE);
    let lockfile = PackageLockfile { dependencies };
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
enum ManifestDependencySpec {
    Path {
        alias: String,
        path: PathBuf,
    },
    Git {
        alias: String,
        git: String,
        rev: String,
    },
    Registry {
        alias: String,
        package: String,
        version_req: String,
    },
}

impl ManifestDependencySpec {
    fn alias(&self) -> &str {
        match self {
            Self::Path { alias, .. } | Self::Git { alias, .. } | Self::Registry { alias, .. } => {
                alias
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LockedDependencySpec {
    Path {
        alias: String,
        path: PathBuf,
    },
    Git {
        alias: String,
        git: String,
        rev: String,
        commit: Option<String>,
    },
    Registry {
        alias: String,
        package: String,
        version: String,
    },
}

impl LockedDependencySpec {
    fn alias(&self) -> &str {
        match self {
            Self::Path { alias, .. } | Self::Git { alias, .. } | Self::Registry { alias, .. } => {
                alias
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageManifest {
    name: String,
    version: Option<String>,
    kind: PackageKind,
    dependencies: Vec<ManifestDependencySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageLockfile {
    dependencies: Vec<LockedDependencySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetectedPackageLayout {
    package_root: PathBuf,
    dependency_store_root: PathBuf,
    source_root: PathBuf,
    modules: Vec<(OwnedModulePath, PathBuf)>,
    manifest: PackageManifest,
    dependencies: Vec<LockedDependencySpec>,
}

fn collect_dependency_packages(
    layout: DetectedPackageLayout,
    module_prefix: OwnedModulePath,
    packages: &mut Vec<PackageLoadPlan>,
    discovery_diagnostics: &mut Vec<ProjectDiscoveryDiagnostic>,
    active_package_roots: &mut HashSet<PathBuf>,
) {
    if !active_package_roots.insert(layout.package_root.clone()) {
        discovery_diagnostics.push(cyclic_dependency_diagnostic(&layout.package_root));
        return;
    }

    discovery_diagnostics.extend(reserved_deps_namespace_diagnostics(&layout.modules));

    let package_root = layout.package_root.clone();
    let dependency_store_root = layout.dependency_store_root.clone();
    let source_root = layout.source_root.clone();
    let entry_file = source_root.join(layout.manifest.kind.entry_file_name());
    let dependencies = layout.dependencies.clone();
    let modules = layout.modules;

    packages.push(PackageLoadPlan {
        package_root: Some(package_root.clone()),
        source_root,
        entry_file,
        module_prefix: module_prefix.clone(),
        modules,
    });

    for dependency in &dependencies {
        match discover_locked_dependency_layout(&package_root, &dependency_store_root, dependency) {
            Ok(layout) => collect_dependency_packages(
                layout,
                dependency_module_prefix(&module_prefix, dependency.alias()),
                packages,
                discovery_diagnostics,
                active_package_roots,
            ),
            Err(diag) => discovery_diagnostics.push(diag),
        }
    }

    active_package_roots.remove(&package_root);
}

fn validate_dependency_graph(
    package_root: &Path,
    dependency_store_root: &Path,
    dependencies: &[LockedDependencySpec],
) -> Result<(), ProjectDiscoveryDiagnostic> {
    let mut active_package_roots = HashSet::from([normalize_existing_path(package_root)]);
    for dependency in dependencies {
        validate_dependency_graph_inner(
            package_root,
            dependency_store_root,
            dependency,
            &mut active_package_roots,
        )?;
    }
    Ok(())
}

fn validate_dependency_graph_inner(
    importing_package_root: &Path,
    dependency_store_root: &Path,
    dependency: &LockedDependencySpec,
    active_package_roots: &mut HashSet<PathBuf>,
) -> Result<(), ProjectDiscoveryDiagnostic> {
    let layout = discover_locked_dependency_layout(
        importing_package_root,
        dependency_store_root,
        dependency,
    )?;
    if !active_package_roots.insert(layout.package_root.clone()) {
        return Err(cyclic_dependency_diagnostic(&layout.package_root));
    }
    for dependency in &layout.dependencies {
        validate_dependency_graph_inner(
            &layout.package_root,
            dependency_store_root,
            dependency,
            active_package_roots,
        )?;
    }
    active_package_roots.remove(&layout.package_root);
    Ok(())
}

fn cyclic_dependency_diagnostic(package_root: &Path) -> ProjectDiscoveryDiagnostic {
    ProjectDiscoveryDiagnostic {
        path: package_root.join(PACKAGE_MANIFEST),
        message: format!(
            "invalid package manifest `{}`: cyclic dependency on `{}`",
            package_root.join(PACKAGE_MANIFEST).display(),
            package_root.display()
        ),
    }
}

fn dependency_module_prefix(parent_prefix: &OwnedModulePath, alias: &str) -> OwnedModulePath {
    OwnedModulePath(vec!["deps".to_owned(), alias.to_owned()]).prefixed(parent_prefix)
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
    let manifest = parse_package_manifest(&manifest_path, &manifest_source)?;
    let package_root = normalize_existing_path(
        manifest_path
            .parent()
            .expect("manifest file should have a parent directory"),
    );
    let dependencies = resolve_dependencies_for_manifest(&manifest_path, &manifest, &package_root)?;

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

    let source_root = package_root.join("src");
    Ok(Some(DetectedPackageLayout {
        modules: discover_module_files(&source_root, entry_file),
        source_root,
        dependency_store_root: package_root.clone(),
        package_root,
        manifest,
        dependencies,
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
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_manifest(manifest_path, "package.name must be a non-empty string"))?
        .to_owned();
    let version = package
        .get("version")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
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
    let dependencies = parse_manifest_dependencies(manifest_path, &manifest)?;

    Ok(PackageManifest {
        name,
        version,
        kind,
        dependencies,
    })
}

fn resolve_dependencies_for_manifest(
    manifest_path: &Path,
    manifest: &PackageManifest,
    dependency_store_root: &Path,
) -> Result<Vec<LockedDependencySpec>, ProjectDiscoveryDiagnostic> {
    let package_root = normalize_existing_path(
        manifest_path
            .parent()
            .expect("manifest file should have a parent directory"),
    );
    let lockfile_path = manifest_path.with_file_name(PACKAGE_LOCKFILE);
    if lockfile_path.is_file()
        && let Ok(lockfile_source) = std::fs::read_to_string(&lockfile_path)
        && let Ok(lockfile) = parse_package_lockfile(&lockfile_path, &lockfile_source)
        && locked_dependencies_match_manifest(&lockfile.dependencies, &manifest.dependencies)
    {
        for dependency in &lockfile.dependencies {
            ensure_locked_dependency_source(&package_root, dependency_store_root, dependency)?;
        }
        return Ok(lockfile.dependencies);
    }

    manifest
        .dependencies
        .iter()
        .map(|dependency| {
            resolve_manifest_dependency(&package_root, dependency_store_root, dependency)
        })
        .collect()
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

    let dependencies = parse_locked_dependencies(lockfile_path, &lockfile)?;
    Ok(PackageLockfile { dependencies })
}

fn render_package_lockfile(lockfile: &PackageLockfile) -> String {
    let mut dependencies = lockfile.dependencies.clone();
    dependencies.sort_by(|lhs, rhs| lhs.alias().cmp(rhs.alias()));

    let mut rendered = format!("version = {LOCKFILE_VERSION}\n\n[dependencies]\n");
    for dependency in dependencies {
        match dependency {
            LockedDependencySpec::Path { alias, path } => {
                rendered.push_str(&format!("{alias} = {{ path = \"{}\" }}\n", path.display()));
            }
            LockedDependencySpec::Git {
                alias,
                git,
                rev,
                commit,
            } => {
                rendered.push_str(&format!(
                    "{alias} = {{ git = \"{git}\", rev = \"{rev}\"{} }}\n",
                    commit
                        .map(|commit| format!(", commit = \"{commit}\""))
                        .unwrap_or_default()
                ));
            }
            LockedDependencySpec::Registry {
                alias,
                package,
                version,
            } => {
                rendered.push_str(&format!(
                    "{alias} = {{ package = \"{package}\", version = \"{version}\" }}\n"
                ));
            }
        }
    }
    rendered
}

fn locked_dependencies_match_manifest(
    locked: &[LockedDependencySpec],
    manifest: &[ManifestDependencySpec],
) -> bool {
    if locked.len() != manifest.len() {
        return false;
    }

    let mut locked_sorted = locked.to_vec();
    let mut manifest_sorted = manifest.to_vec();
    locked_sorted.sort_by(|lhs, rhs| lhs.alias().cmp(rhs.alias()));
    manifest_sorted.sort_by(|lhs, rhs| lhs.alias().cmp(rhs.alias()));

    locked_sorted
        .iter()
        .zip(manifest_sorted.iter())
        .all(|(locked, manifest)| manifest_dependency_matches_locked(manifest, locked))
}

fn manifest_dependency_matches_locked(
    manifest: &ManifestDependencySpec,
    locked: &LockedDependencySpec,
) -> bool {
    match (manifest, locked) {
        (
            ManifestDependencySpec::Path {
                alias: manifest_alias,
                path,
            },
            LockedDependencySpec::Path {
                alias: locked_alias,
                path: locked_path,
            },
        ) => manifest_alias == locked_alias && path == locked_path,
        (
            ManifestDependencySpec::Git {
                alias: manifest_alias,
                git,
                rev,
            },
            LockedDependencySpec::Git {
                alias: locked_alias,
                git: locked_git,
                rev: locked_rev,
                ..
            },
        ) => manifest_alias == locked_alias && git == locked_git && rev == locked_rev,
        (
            ManifestDependencySpec::Registry {
                alias: manifest_alias,
                package,
                version_req,
            },
            LockedDependencySpec::Registry {
                alias: locked_alias,
                package: locked_package,
                version,
            },
        ) => {
            manifest_alias == locked_alias
                && package == locked_package
                && VersionReq::parse(version_req)
                    .ok()
                    .zip(Version::parse(version).ok())
                    .is_some_and(|(req, version)| req.matches(&version))
        }
        _ => false,
    }
}

fn parse_manifest_dependencies(
    manifest_path: &Path,
    manifest: &toml::Value,
) -> Result<Vec<ManifestDependencySpec>, ProjectDiscoveryDiagnostic> {
    let Some(dependencies) = manifest.get("dependencies") else {
        return Ok(Vec::new());
    };
    let deps_table = dependencies.as_table().ok_or_else(|| {
        invalid_manifest(
            manifest_path,
            "[dependencies] must be a TOML table of alias = { ... } entries",
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
            .filter(|value| !value.is_empty());
        let git = spec_table
            .get("git")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let rev = spec_table
            .get("rev")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let package = spec_table
            .get("package")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let version = spec_table
            .get("version")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());

        let dependency = match (path, git, rev, package, version) {
            (Some(path), None, None, None, None) => ManifestDependencySpec::Path {
                alias: alias.clone(),
                path: PathBuf::from(path),
            },
            (None, Some(git), Some(rev), None, None) => ManifestDependencySpec::Git {
                alias: alias.clone(),
                git: git.to_owned(),
                rev: rev.to_owned(),
            },
            (None, None, None, Some(package), Some(version_req)) => {
                ManifestDependencySpec::Registry {
                    alias: alias.clone(),
                    package: package.to_owned(),
                    version_req: version_req.to_owned(),
                }
            }
            _ => {
                return Err(invalid_manifest(
                    manifest_path,
                    format!(
                        "dependency `{alias}` must use exactly one source form: {{ path = ... }}, {{ git = ..., rev = ... }}, or {{ package = ..., version = ... }}"
                    ),
                ));
            }
        };

        parsed.push(dependency);
    }

    Ok(parsed)
}

fn parse_locked_dependencies(
    manifest_path: &Path,
    manifest: &toml::Value,
) -> Result<Vec<LockedDependencySpec>, ProjectDiscoveryDiagnostic> {
    let Some(dependencies) = manifest.get("dependencies") else {
        return Ok(Vec::new());
    };
    let deps_table = dependencies.as_table().ok_or_else(|| {
        invalid_manifest(
            manifest_path,
            "[dependencies] must be a TOML table of alias = { ... } entries",
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
            .filter(|value| !value.is_empty());
        let git = spec_table
            .get("git")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let rev = spec_table
            .get("rev")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let commit = spec_table
            .get("commit")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let package = spec_table
            .get("package")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());
        let version = spec_table
            .get("version")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.is_empty());

        let dependency = match (path, git, rev, commit, package, version) {
            (Some(path), None, None, None, None, None) => LockedDependencySpec::Path {
                alias: alias.clone(),
                path: PathBuf::from(path),
            },
            (None, Some(git), Some(rev), commit, None, None) => LockedDependencySpec::Git {
                alias: alias.clone(),
                git: git.to_owned(),
                rev: rev.to_owned(),
                commit: commit.map(str::to_owned),
            },
            (None, None, None, None, Some(package), Some(version)) => {
                LockedDependencySpec::Registry {
                    alias: alias.clone(),
                    package: package.to_owned(),
                    version: version.to_owned(),
                }
            }
            _ => {
                return Err(invalid_manifest(
                    manifest_path,
                    format!(
                        "dependency `{alias}` must use exact lockfile source form: {{ path = ... }}, {{ git = ..., rev = ... }}, or {{ package = ..., version = ... }}"
                    ),
                ));
            }
        };

        parsed.push(dependency);
    }

    Ok(parsed)
}

fn resolve_manifest_dependency(
    package_root: &Path,
    dependency_store_root: &Path,
    dependency: &ManifestDependencySpec,
) -> Result<LockedDependencySpec, ProjectDiscoveryDiagnostic> {
    match dependency {
        ManifestDependencySpec::Path { alias, path } => Ok(LockedDependencySpec::Path {
            alias: alias.clone(),
            path: path.clone(),
        }),
        ManifestDependencySpec::Git { alias, git, rev } => {
            let commit = resolve_and_cache_git_dependency(
                package_root,
                alias,
                git,
                rev,
                dependency_store_root,
            )?;
            Ok(LockedDependencySpec::Git {
                alias: alias.clone(),
                git: git.clone(),
                rev: rev.clone(),
                commit: Some(commit),
            })
        }
        ManifestDependencySpec::Registry {
            alias,
            package,
            version_req,
        } => {
            let version = resolve_registry_dependency_version(
                dependency_store_root,
                package_root,
                alias,
                package,
                version_req,
            )?;
            Ok(LockedDependencySpec::Registry {
                alias: alias.clone(),
                package: package.clone(),
                version,
            })
        }
    }
}

fn ensure_locked_dependency_source(
    package_root: &Path,
    dependency_store_root: &Path,
    dependency: &LockedDependencySpec,
) -> Result<(), ProjectDiscoveryDiagnostic> {
    match dependency {
        LockedDependencySpec::Path { .. } => Ok(()),
        LockedDependencySpec::Git {
            alias,
            git,
            rev,
            commit,
        } => {
            let checkout_ref = commit.as_deref().unwrap_or(rev);
            let checkout_root =
                git_dependency_checkout_root(dependency_store_root, git, checkout_ref);
            sync_git_dependency_checkout(package_root, alias, git, checkout_ref, &checkout_root)
        }
        LockedDependencySpec::Registry {
            alias,
            package,
            version,
        } => {
            let package_root = registry_dependency_root(dependency_store_root, package, version);
            if !package_root.join(PACKAGE_MANIFEST).is_file() {
                return Err(invalid_manifest(
                    &package_root.join(PACKAGE_MANIFEST),
                    format!(
                        "dependency `{alias}` could not find registry package `{package}` version `{version}`"
                    ),
                ));
            }
            Ok(())
        }
    }
}

fn git_dependency_checkout_root(package_root: &Path, git: &str, rev: &str) -> PathBuf {
    package_root
        .join(".kyokara")
        .join("git")
        .join(git_dependency_cache_key(git, rev))
}

fn git_dependency_cache_key(git: &str, rev: &str) -> String {
    let mut hasher = DefaultHasher::new();
    git.hash(&mut hasher);
    "\0".hash(&mut hasher);
    rev.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn resolve_and_cache_git_dependency(
    package_root: &Path,
    alias: &str,
    git: &str,
    rev: &str,
    dependency_store_root: &Path,
) -> Result<String, ProjectDiscoveryDiagnostic> {
    let parent = dependency_store_root.join(".kyokara").join("git");
    std::fs::create_dir_all(&parent).map_err(|err| {
        invalid_manifest(
            &package_root.join(PACKAGE_MANIFEST),
            format!("failed to prepare git dependency `{alias}` cache: {err}"),
        )
    })?;

    let temp_root = parent.join(format!(".tmp-{}", git_dependency_cache_key(git, rev)));
    let _ = std::fs::remove_dir_all(&temp_root);

    let temp_root_str = temp_root.to_string_lossy().into_owned();
    run_git_command(
        &package_root.join(PACKAGE_MANIFEST),
        &["clone", "--quiet", git, temp_root_str.as_str()],
    )?;
    run_git_command(
        &package_root.join(PACKAGE_MANIFEST),
        &["-C", temp_root_str.as_str(), "checkout", "--quiet", rev],
    )?;
    let resolved_commit = git_head_commit(&package_root.join(PACKAGE_MANIFEST), &temp_root)?;
    let checkout_root = git_dependency_checkout_root(dependency_store_root, git, &resolved_commit);
    if checkout_root.join(PACKAGE_MANIFEST).is_file() {
        let _ = std::fs::remove_dir_all(&temp_root);
        return Ok(resolved_commit);
    }

    let _ = std::fs::remove_dir_all(temp_root.join(".git"));
    let _ = std::fs::remove_dir_all(&checkout_root);
    std::fs::rename(&temp_root, &checkout_root).map_err(|err| {
        invalid_manifest(
            &package_root.join(PACKAGE_MANIFEST),
            format!("failed to finalize git dependency `{alias}` checkout: {err}"),
        )
    })?;
    Ok(resolved_commit)
}

fn sync_git_dependency_checkout(
    package_root: &Path,
    alias: &str,
    git: &str,
    rev: &str,
    checkout_root: &Path,
) -> Result<(), ProjectDiscoveryDiagnostic> {
    if checkout_root.join(PACKAGE_MANIFEST).is_file() {
        return Ok(());
    }

    let parent = checkout_root
        .parent()
        .expect("git checkout root should have a parent");
    std::fs::create_dir_all(parent).map_err(|err| {
        invalid_manifest(
            &package_root.join(PACKAGE_MANIFEST),
            format!("failed to prepare git dependency `{alias}` cache: {err}"),
        )
    })?;

    let temp_root = parent.join(format!(".tmp-{}", git_dependency_cache_key(git, rev)));
    let _ = std::fs::remove_dir_all(&temp_root);

    let temp_root_str = temp_root.to_string_lossy().into_owned();
    run_git_command(
        &package_root.join(PACKAGE_MANIFEST),
        &["clone", "--quiet", git, temp_root_str.as_str()],
    )?;
    run_git_command(
        &package_root.join(PACKAGE_MANIFEST),
        &["-C", temp_root_str.as_str(), "checkout", "--quiet", rev],
    )?;

    let _ = std::fs::remove_dir_all(temp_root.join(".git"));
    let _ = std::fs::remove_dir_all(checkout_root);
    std::fs::rename(&temp_root, checkout_root).map_err(|err| {
        invalid_manifest(
            &package_root.join(PACKAGE_MANIFEST),
            format!("failed to finalize git dependency `{alias}` checkout: {err}"),
        )
    })?;

    Ok(())
}

fn git_head_commit(
    manifest_path: &Path,
    checkout_root: &Path,
) -> Result<String, ProjectDiscoveryDiagnostic> {
    let output = Command::new("git")
        .args(["-C", &checkout_root.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .map_err(|err| {
            invalid_manifest(
                manifest_path,
                format!("failed to read git dependency commit: {err}"),
            )
        })?;
    if !output.status.success() {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "git command `-C {} rev-parse HEAD` failed: {}",
                checkout_root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git_command(manifest_path: &Path, args: &[&str]) -> Result<(), ProjectDiscoveryDiagnostic> {
    let output = Command::new("git").args(args).output().map_err(|err| {
        invalid_manifest(
            manifest_path,
            format!("failed to run git command `{}`: {err}", args.join(" ")),
        )
    })?;
    if !output.status.success() {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "git command `{}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(())
}

fn resolve_registry_dependency_version(
    dependency_store_root: &Path,
    manifest_package_root: &Path,
    alias: &str,
    package: &str,
    version_req: &str,
) -> Result<String, ProjectDiscoveryDiagnostic> {
    let req = VersionReq::parse(version_req).map_err(|err| {
        invalid_manifest(
            &manifest_package_root.join(PACKAGE_MANIFEST),
            format!("dependency `{alias}` has invalid version requirement `{version_req}`: {err}"),
        )
    })?;
    let package_dir = dependency_store_root
        .join(".kyokara")
        .join("registry")
        .join("packages")
        .join(package);
    let entries = std::fs::read_dir(&package_dir).map_err(|err| {
        invalid_manifest(
            &manifest_package_root.join(PACKAGE_MANIFEST),
            format!("dependency `{alias}` could not find registry package `{package}`: {err}"),
        )
    })?;

    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            invalid_manifest(
                &manifest_package_root.join(PACKAGE_MANIFEST),
                format!("failed to read registry package `{package}`: {err}"),
            )
        })?;
        let raw_version = entry.file_name().to_string_lossy().to_string();
        let Ok(version) = Version::parse(&raw_version) else {
            continue;
        };
        if req.matches(&version) {
            candidates.push(version);
        }
    }
    candidates.sort();
    let Some(version) = candidates.pop() else {
        return Err(invalid_manifest(
            &manifest_package_root.join(PACKAGE_MANIFEST),
            format!(
                "dependency `{alias}` could not resolve `{package}` for version requirement `{version_req}`"
            ),
        ));
    };
    Ok(version.to_string())
}

fn discover_locked_dependency_layout(
    importing_package_root: &Path,
    dependency_store_root: &Path,
    dependency: &LockedDependencySpec,
) -> Result<DetectedPackageLayout, ProjectDiscoveryDiagnostic> {
    ensure_locked_dependency_source(importing_package_root, dependency_store_root, dependency)?;
    let package_root =
        locked_dependency_package_root(importing_package_root, dependency_store_root, dependency);
    let manifest_path = package_root.join(PACKAGE_MANIFEST);
    if !manifest_path.is_file() {
        return Err(ProjectDiscoveryDiagnostic {
            path: manifest_path,
            message: format!(
                "invalid package manifest `{}`: dependency `{}` is missing kyokara.toml",
                package_root.join(PACKAGE_MANIFEST).display(),
                dependency.alias()
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
                dependency.alias()
            ),
        });
    }

    let dependencies =
        resolve_dependencies_for_manifest(&manifest_path, &manifest, dependency_store_root)?;

    Ok(DetectedPackageLayout {
        package_root,
        dependency_store_root: dependency_store_root.to_path_buf(),
        source_root: source_root.clone(),
        modules: discover_module_files(&source_root, &entry_file),
        manifest,
        dependencies,
    })
}

fn locked_dependency_package_root(
    importing_package_root: &Path,
    dependency_store_root: &Path,
    dependency: &LockedDependencySpec,
) -> PathBuf {
    match dependency {
        LockedDependencySpec::Path { path, .. } => {
            normalize_path(&importing_package_root.join(path))
        }
        LockedDependencySpec::Git {
            git, rev, commit, ..
        } => normalize_path(&git_dependency_checkout_root(
            dependency_store_root,
            git,
            commit.as_deref().unwrap_or(rev),
        )),
        LockedDependencySpec::Registry {
            package, version, ..
        } => normalize_path(&registry_dependency_root(
            dependency_store_root,
            package,
            version,
        )),
    }
}

fn registry_dependency_root(dependency_store_root: &Path, package: &str, version: &str) -> PathBuf {
    dependency_store_root
        .join(".kyokara")
        .join("registry")
        .join("packages")
        .join(package)
        .join(version)
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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
