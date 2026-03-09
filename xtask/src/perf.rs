use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, PerfError>;

const SCHEMA_VERSION: u32 = 1;
const PROFILE_NAME: &str = "release-lto-fat";

#[derive(Debug, Error)]
pub enum PerfError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid benchmark manifest in `{case_id}`: {message}")]
    InvalidManifest { case_id: String, message: String },
    #[error("missing matching baseline; run `cargo run -p xtask -- perf record` on this machine")]
    MissingMatchingBaseline,
    #[error("multiple matching baselines found for current machine")]
    MultipleMatchingBaselines,
    #[error("baseline case set mismatch: {message}")]
    BaselineMismatch { message: String },
    #[error("unknown benchmark case `{0}`")]
    UnknownCase(String),
    #[error("performance regression: {message}")]
    Regression { message: String },
    #[error("command `{command}` failed with status {status}: {stderr}")]
    CommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },
    #[error("unexpected stdout for `{case_id}`: expected {expected:?}, got {actual:?}")]
    UnexpectedStdout {
        case_id: String,
        expected: String,
        actual: String,
    },
    #[error("invalid check output for `{case_id}`: {message}")]
    InvalidCheckOutput { case_id: String, message: String },
    #[error("workspace root is not the parent of `xtask`")]
    WorkspaceRootUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BenchMode {
    Run,
    Check,
}

impl fmt::Display for BenchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run => f.write_str("run"),
            Self::Check => f.write_str("check"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchManifest {
    pub id: String,
    pub mode: BenchMode,
    pub entry: String,
    #[serde(default)]
    pub project: bool,
    #[serde(default)]
    pub expected_stdout: Option<String>,
    #[serde(default)]
    pub expected_ok: Option<bool>,
    pub warmup_runs: usize,
    pub measured_runs: usize,
    pub max_regression_pct: f64,
    pub max_regression_ms: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchCase {
    pub dir: PathBuf,
    pub manifest: BenchManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentFingerprint {
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub rustc: String,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineCaseResult {
    pub id: String,
    pub mode: BenchMode,
    pub samples_ms: Vec<f64>,
    pub median_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineFile {
    pub schema_version: u32,
    pub fingerprint: EnvironmentFingerprint,
    pub results: Vec<BaselineCaseResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Recorded,
    Pass,
    Regression,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatestCaseReport {
    pub id: String,
    pub mode: BenchMode,
    pub status: CaseStatus,
    pub samples_ms: Vec<f64>,
    pub median_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_median_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatestReport {
    pub schema_version: u32,
    pub command: String,
    pub fingerprint: EnvironmentFingerprint,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_file: Option<String>,
    pub success: bool,
    pub cases: Vec<LatestCaseReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    pub cases: Vec<LatestCaseReport>,
}

#[derive(Debug, Clone)]
pub struct PerfPaths {
    pub workspace_root: PathBuf,
    pub binary_path: Option<PathBuf>,
    pub cases_root: PathBuf,
    pub baselines_root: PathBuf,
    pub latest_report_path: PathBuf,
    pub fingerprint_override: Option<EnvironmentFingerprint>,
}

impl PerfPaths {
    pub fn from_workspace_root(workspace_root: PathBuf) -> Self {
        let target_dir = workspace_root.join("target").join("perf");
        Self {
            binary_path: None,
            cases_root: workspace_root.join("tools").join("perf").join("cases"),
            baselines_root: workspace_root.join("tools").join("perf").join("baselines"),
            latest_report_path: target_dir.join("latest.json"),
            fingerprint_override: None,
            workspace_root,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerfConfig {
    pub case_filter: Option<String>,
    pub skip_build: bool,
}

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Kyokara build and maintenance tasks")]
struct XtCli {
    #[command(subcommand)]
    command: XtCommand,
}

#[derive(Debug, Subcommand)]
enum XtCommand {
    /// Run the performance harness.
    Perf(PerfArgs),
}

#[derive(Debug, Args)]
struct PerfArgs {
    #[command(subcommand)]
    command: PerfCommand,
}

#[derive(Debug, Subcommand)]
enum PerfCommand {
    /// Record current performance as the baseline for this machine fingerprint.
    Record {
        #[arg(long)]
        case: Option<String>,
    },
    /// Check current performance against the committed baseline for this machine fingerprint.
    Check {
        #[arg(long)]
        case: Option<String>,
    },
}

pub fn dispatch(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let cli = XtCli::parse_from(args);
    let workspace_root = workspace_root()?;
    let paths = PerfPaths::from_workspace_root(workspace_root);
    match cli.command {
        XtCommand::Perf(PerfArgs { command }) => match command {
            PerfCommand::Record { case } => {
                let _report = record(
                    &paths,
                    &PerfConfig {
                        case_filter: case,
                        skip_build: false,
                    },
                )?;
                Ok(())
            }
            PerfCommand::Check { case } => {
                let _report = check(
                    &paths,
                    &PerfConfig {
                        case_filter: case,
                        skip_build: false,
                    },
                )?;
                Ok(())
            }
        },
    }
}

pub fn discover_cases(cases_root: &Path) -> Result<Vec<BenchCase>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(cases_root)? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();

    let mut cases = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let manifest_path = dir.join("bench.json");
        let manifest: BenchManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        validate_manifest(&manifest, &dir)?;
        cases.push(BenchCase { dir, manifest });
    }
    Ok(cases)
}

pub fn save_baseline(path: &Path, baseline: &BaselineFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(baseline)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn load_baseline(path: &Path) -> Result<BaselineFile> {
    let json = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

pub fn compare_against_baseline(
    manifests: &[BenchCase],
    baseline: &BaselineFile,
    current: &BaselineFile,
) -> Result<CheckOutcome> {
    let (cases, has_regression) = classify_against_baseline(manifests, baseline, current, true)?;
    if has_regression {
        let offenders = regression_case_ids(&cases);
        return Err(PerfError::Regression {
            message: format!("{} exceeded the configured threshold", offenders.join(", ")),
        });
    }
    Ok(CheckOutcome { cases })
}

pub fn record(paths: &PerfPaths, config: &PerfConfig) -> Result<LatestReport> {
    let binary_path = resolve_binary_path(paths, config.skip_build)?;
    let fingerprint = resolve_fingerprint(paths)?;
    let cases = select_cases(
        discover_cases(&paths.cases_root)?,
        config.case_filter.as_deref(),
    )?;
    let current = run_cases(paths, &binary_path, &cases, &fingerprint)?;

    let baseline_path = expected_baseline_path(paths, &fingerprint);
    let baseline = if config.case_filter.is_some() {
        match find_matching_baseline(paths, &fingerprint)? {
            Some((_, existing)) => merge_baseline(existing, &current),
            None => current.clone(),
        }
    } else {
        current.clone()
    };
    save_baseline(&baseline_path, &baseline)?;

    let report = LatestReport {
        schema_version: SCHEMA_VERSION,
        command: "record".into(),
        fingerprint,
        baseline_file: Some(baseline_path.display().to_string()),
        success: true,
        cases: current
            .results
            .iter()
            .map(|result| LatestCaseReport {
                id: result.id.clone(),
                mode: result.mode,
                status: CaseStatus::Recorded,
                samples_ms: result.samples_ms.clone(),
                median_ms: result.median_ms,
                baseline_median_ms: None,
            })
            .collect(),
    };
    write_latest_report(paths, &report)?;
    print_report(&report);
    Ok(report)
}

pub fn check(paths: &PerfPaths, config: &PerfConfig) -> Result<LatestReport> {
    let binary_path = resolve_binary_path(paths, config.skip_build)?;
    let fingerprint = resolve_fingerprint(paths)?;
    let cases = select_cases(
        discover_cases(&paths.cases_root)?,
        config.case_filter.as_deref(),
    )?;
    let current = run_cases(paths, &binary_path, &cases, &fingerprint)?;
    let strict_case_set = config.case_filter.is_none();

    let Some((baseline_path, baseline)) = find_matching_baseline(paths, &fingerprint)? else {
        let report = LatestReport {
            schema_version: SCHEMA_VERSION,
            command: "check".into(),
            fingerprint,
            baseline_file: None,
            success: false,
            cases: current
                .results
                .iter()
                .map(|result| LatestCaseReport {
                    id: result.id.clone(),
                    mode: result.mode,
                    status: CaseStatus::Recorded,
                    samples_ms: result.samples_ms.clone(),
                    median_ms: result.median_ms,
                    baseline_median_ms: None,
                })
                .collect(),
        };
        write_latest_report(paths, &report)?;
        print_report(&report);
        return Err(PerfError::MissingMatchingBaseline);
    };

    let (case_reports, has_regression) =
        classify_against_baseline(&cases, &baseline, &current, strict_case_set)?;

    let report = LatestReport {
        schema_version: SCHEMA_VERSION,
        command: "check".into(),
        fingerprint,
        baseline_file: Some(baseline_path.display().to_string()),
        success: !has_regression,
        cases: case_reports,
    };
    write_latest_report(paths, &report)?;
    print_report(&report);

    if has_regression {
        return Err(PerfError::Regression {
            message: format!(
                "{} exceeded the configured threshold",
                regression_case_ids(&report.cases).join(", ")
            ),
        });
    }

    Ok(report)
}

fn expected_baseline_path(paths: &PerfPaths, fingerprint: &EnvironmentFingerprint) -> PathBuf {
    let slug = [
        sanitize_baseline_component(&fingerprint.os),
        sanitize_baseline_component(&fingerprint.arch),
        sanitize_baseline_component(&fingerprint.cpu_model),
        sanitize_baseline_component(&fingerprint.rustc),
        sanitize_baseline_component(&fingerprint.profile),
    ]
    .join("__");
    paths.baselines_root.join(format!("{slug}.json"))
}

fn sanitize_baseline_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or(PerfError::WorkspaceRootUnavailable)
}

fn validate_manifest(manifest: &BenchManifest, dir: &Path) -> Result<()> {
    let case_id = manifest.id.clone();
    let dir_name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>");
    if manifest.id != dir_name {
        return Err(PerfError::InvalidManifest {
            case_id,
            message: format!("manifest id must match directory name `{dir_name}`"),
        });
    }
    if manifest.entry.trim().is_empty() {
        return Err(PerfError::InvalidManifest {
            case_id,
            message: "entry must not be empty".into(),
        });
    }
    if !dir.join(&manifest.entry).exists() {
        return Err(PerfError::InvalidManifest {
            case_id,
            message: format!("entry `{}` does not exist", manifest.entry),
        });
    }
    if manifest.warmup_runs == 0 {
        return Err(PerfError::InvalidManifest {
            case_id,
            message: "warmup_runs must be at least 1".into(),
        });
    }
    if manifest.measured_runs == 0 {
        return Err(PerfError::InvalidManifest {
            case_id,
            message: "measured_runs must be at least 1".into(),
        });
    }
    if manifest.max_regression_pct < 0.0 || manifest.max_regression_ms < 0.0 {
        return Err(PerfError::InvalidManifest {
            case_id,
            message: "regression thresholds must be non-negative".into(),
        });
    }

    match manifest.mode {
        BenchMode::Run => {
            if manifest.expected_stdout.is_none() {
                return Err(PerfError::InvalidManifest {
                    case_id,
                    message: "run case requires expected_stdout".into(),
                });
            }
            if manifest.expected_ok.is_some() {
                return Err(PerfError::InvalidManifest {
                    case_id,
                    message: "run case must not set expected_ok".into(),
                });
            }
        }
        BenchMode::Check => {
            if manifest.expected_ok != Some(true) {
                return Err(PerfError::InvalidManifest {
                    case_id,
                    message: "check case requires expected_ok = true".into(),
                });
            }
            if manifest.expected_stdout.is_some() {
                return Err(PerfError::InvalidManifest {
                    case_id,
                    message: "check case must not set expected_stdout".into(),
                });
            }
        }
    }
    Ok(())
}

fn select_cases(cases: Vec<BenchCase>, case_filter: Option<&str>) -> Result<Vec<BenchCase>> {
    match case_filter {
        Some(filter) => {
            let selected: Vec<_> = cases
                .into_iter()
                .filter(|case| case.manifest.id == filter)
                .collect();
            if selected.is_empty() {
                return Err(PerfError::UnknownCase(filter.to_string()));
            }
            Ok(selected)
        }
        None => Ok(cases),
    }
}

fn resolve_binary_path(paths: &PerfPaths, skip_build: bool) -> Result<PathBuf> {
    if let Some(path) = &paths.binary_path {
        return Ok(path.clone());
    }
    if !skip_build {
        let output = Command::new("cargo")
            .args(["build", "--release", "-p", "kyokara-cli"])
            .current_dir(&paths.workspace_root)
            .output()?;
        if !output.status.success() {
            return Err(PerfError::CommandFailed {
                command: "cargo build --release -p kyokara-cli".into(),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
    }
    Ok(paths
        .workspace_root
        .join("target")
        .join("release")
        .join(if cfg!(windows) {
            "kyokara.exe"
        } else {
            "kyokara"
        }))
}

fn resolve_fingerprint(paths: &PerfPaths) -> Result<EnvironmentFingerprint> {
    if let Some(fingerprint) = &paths.fingerprint_override {
        return Ok(fingerprint.clone());
    }

    let rustc = capture_command("rustc", &["-V"])?.trim().to_string();
    Ok(EnvironmentFingerprint {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_model: detect_cpu_model(),
        rustc,
        profile: PROFILE_NAME.into(),
    })
}

fn capture_command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(PerfError::CommandFailed {
            command: format!("{program} {}", args.join(" ")).trim().to_string(),
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn detect_cpu_model() -> String {
    if cfg!(target_os = "macos") {
        for args in [
            ["-n", "machdep.cpu.brand_string"].as_slice(),
            ["-n", "hw.model"].as_slice(),
            ["-n", "hw.machine"].as_slice(),
        ] {
            if let Ok(output) = capture_command("sysctl", args) {
                let value = output.trim();
                if !value.is_empty() {
                    return value.to_string();
                }
            }
        }
    }
    if cfg!(target_os = "linux")
        && let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo")
        && let Some(line) = cpuinfo
            .lines()
            .find(|line| line.starts_with("model name") || line.starts_with("Hardware"))
        && let Some((_, value)) = line.split_once(':')
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn run_cases(
    paths: &PerfPaths,
    binary_path: &Path,
    cases: &[BenchCase],
    fingerprint: &EnvironmentFingerprint,
) -> Result<BaselineFile> {
    let mut results = Vec::with_capacity(cases.len());
    for case in cases {
        for _ in 0..case.manifest.warmup_runs {
            let _ = run_case_once(paths, binary_path, case)?;
        }
        let mut samples_ms = Vec::with_capacity(case.manifest.measured_runs);
        for _ in 0..case.manifest.measured_runs {
            samples_ms.push(run_case_once(paths, binary_path, case)?);
        }
        results.push(BaselineCaseResult {
            id: case.manifest.id.clone(),
            mode: case.manifest.mode,
            median_ms: median_ms(&samples_ms),
            samples_ms,
        });
    }
    Ok(BaselineFile {
        schema_version: SCHEMA_VERSION,
        fingerprint: fingerprint.clone(),
        results,
    })
}

fn run_case_once(paths: &PerfPaths, binary_path: &Path, case: &BenchCase) -> Result<f64> {
    let entry_path = case.dir.join(&case.manifest.entry);
    let mut command = Command::new(binary_path);
    match case.manifest.mode {
        BenchMode::Run => {
            command.arg("run");
            if case.manifest.project {
                command.arg("--project");
            }
            command.arg(&entry_path);
        }
        BenchMode::Check => {
            command.args(["check", "--format", "json"]);
            if case.manifest.project {
                command.arg("--project");
            }
            command.arg(&entry_path);
        }
    }
    command.current_dir(&paths.workspace_root);

    let rendered = render_command(binary_path, &command);
    let started = Instant::now();
    let output = command.output()?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        return Err(PerfError::CommandFailed {
            command: rendered,
            status: output.status.code().unwrap_or(-1),
            stderr,
        });
    }

    match case.manifest.mode {
        BenchMode::Run => {
            let expected = case.manifest.expected_stdout.clone().unwrap_or_default();
            if stdout != expected {
                return Err(PerfError::UnexpectedStdout {
                    case_id: case.manifest.id.clone(),
                    expected,
                    actual: stdout,
                });
            }
        }
        BenchMode::Check => {
            #[derive(Deserialize)]
            struct CheckJson {
                diagnostics: Vec<serde_json::Value>,
            }

            let parsed: CheckJson =
                serde_json::from_str(&stdout).map_err(|err| PerfError::InvalidCheckOutput {
                    case_id: case.manifest.id.clone(),
                    message: format!("invalid json: {err}"),
                })?;
            if !parsed.diagnostics.is_empty() {
                return Err(PerfError::InvalidCheckOutput {
                    case_id: case.manifest.id.clone(),
                    message: format!(
                        "expected zero diagnostics, got {}",
                        parsed.diagnostics.len()
                    ),
                });
            }
            if case.manifest.expected_ok != Some(true) {
                return Err(PerfError::InvalidCheckOutput {
                    case_id: case.manifest.id.clone(),
                    message: "expected_ok must be true".into(),
                });
            }
        }
    }

    Ok(elapsed_ms)
}

fn render_command(binary_path: &Path, command: &Command) -> String {
    let mut parts = vec![binary_path.display().to_string()];
    parts.extend(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned()),
    );
    parts.join(" ")
}

fn median_ms(samples_ms: &[f64]) -> f64 {
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn find_matching_baseline(
    paths: &PerfPaths,
    fingerprint: &EnvironmentFingerprint,
) -> Result<Option<(PathBuf, BaselineFile)>> {
    let Ok(entries) = fs::read_dir(&paths.baselines_root) else {
        return Ok(None);
    };
    let mut matches = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let baseline = load_baseline(&path)?;
        if baseline.fingerprint == *fingerprint {
            matches.push((path, baseline));
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(PerfError::MultipleMatchingBaselines),
    }
}

fn merge_baseline(existing: BaselineFile, current: &BaselineFile) -> BaselineFile {
    let mut merged: BTreeMap<String, BaselineCaseResult> = existing
        .results
        .into_iter()
        .map(|result| (result.id.clone(), result))
        .collect();
    for result in &current.results {
        merged.insert(result.id.clone(), result.clone());
    }
    BaselineFile {
        schema_version: SCHEMA_VERSION,
        fingerprint: current.fingerprint.clone(),
        results: merged.into_values().collect(),
    }
}

fn classify_against_baseline(
    manifests: &[BenchCase],
    baseline: &BaselineFile,
    current: &BaselineFile,
    strict_case_set: bool,
) -> Result<(Vec<LatestCaseReport>, bool)> {
    if baseline.schema_version != SCHEMA_VERSION || current.schema_version != SCHEMA_VERSION {
        return Err(PerfError::BaselineMismatch {
            message: format!(
                "expected schema version {SCHEMA_VERSION}, got baseline={} current={}",
                baseline.schema_version, current.schema_version
            ),
        });
    }
    if baseline.fingerprint != current.fingerprint {
        return Err(PerfError::BaselineMismatch {
            message: "fingerprint mismatch".into(),
        });
    }

    let expected_ids: BTreeSet<String> = manifests
        .iter()
        .map(|case| case.manifest.id.clone())
        .collect();
    let baseline_map = result_map(&baseline.results);
    let current_map = result_map(&current.results);

    if strict_case_set {
        let baseline_ids: BTreeSet<String> = baseline_map.keys().cloned().collect();
        let current_ids: BTreeSet<String> = current_map.keys().cloned().collect();
        if baseline_ids != expected_ids || current_ids != expected_ids {
            return Err(PerfError::BaselineMismatch {
                message: format!(
                    "expected {:?}, baseline has {:?}, current has {:?}",
                    expected_ids, baseline_ids, current_ids
                ),
            });
        }
    } else {
        let missing_from_baseline: Vec<_> = expected_ids
            .iter()
            .filter(|id| !baseline_map.contains_key(*id))
            .cloned()
            .collect();
        let missing_from_current: Vec<_> = expected_ids
            .iter()
            .filter(|id| !current_map.contains_key(*id))
            .cloned()
            .collect();
        if !missing_from_baseline.is_empty() || !missing_from_current.is_empty() {
            return Err(PerfError::BaselineMismatch {
                message: format!(
                    "missing selected cases: baseline={:?}, current={:?}",
                    missing_from_baseline, missing_from_current
                ),
            });
        }
    }

    let mut reports = Vec::with_capacity(manifests.len());
    let mut has_regression = false;
    for case in manifests {
        let case_id = &case.manifest.id;
        let baseline_result =
            baseline_map
                .get(case_id)
                .ok_or_else(|| PerfError::BaselineMismatch {
                    message: format!("baseline missing `{case_id}`"),
                })?;
        let current_result =
            current_map
                .get(case_id)
                .ok_or_else(|| PerfError::BaselineMismatch {
                    message: format!("current results missing `{case_id}`"),
                })?;
        if baseline_result.mode != case.manifest.mode || current_result.mode != case.manifest.mode {
            return Err(PerfError::BaselineMismatch {
                message: format!("mode mismatch for `{case_id}`"),
            });
        }

        let delta_ms = current_result.median_ms - baseline_result.median_ms;
        let delta_pct = if baseline_result.median_ms == 0.0 {
            if current_result.median_ms == 0.0 {
                0.0
            } else {
                f64::INFINITY
            }
        } else {
            ((current_result.median_ms / baseline_result.median_ms) - 1.0) * 100.0
        };
        let regression = delta_ms > case.manifest.max_regression_ms
            && delta_pct > case.manifest.max_regression_pct;
        has_regression |= regression;

        reports.push(LatestCaseReport {
            id: case_id.clone(),
            mode: case.manifest.mode,
            status: if regression {
                CaseStatus::Regression
            } else {
                CaseStatus::Pass
            },
            samples_ms: current_result.samples_ms.clone(),
            median_ms: current_result.median_ms,
            baseline_median_ms: Some(baseline_result.median_ms),
        });
    }

    Ok((reports, has_regression))
}

fn result_map(results: &[BaselineCaseResult]) -> BTreeMap<String, BaselineCaseResult> {
    results
        .iter()
        .cloned()
        .map(|result| (result.id.clone(), result))
        .collect()
}

fn regression_case_ids(cases: &[LatestCaseReport]) -> Vec<String> {
    cases
        .iter()
        .filter(|case| case.status == CaseStatus::Regression)
        .map(|case| case.id.clone())
        .collect()
}

fn write_latest_report(paths: &PerfPaths, report: &LatestReport) -> Result<()> {
    if let Some(parent) = paths.latest_report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(report)?;
    fs::write(&paths.latest_report_path, json)?;
    Ok(())
}

fn print_report(report: &LatestReport) {
    println!(
        "{:<30} {:<8} {:>12} {:>12} {:<12}",
        "case", "mode", "median_ms", "baseline", "status"
    );
    for case in &report.cases {
        let baseline = case
            .baseline_median_ms
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<30} {:<8} {:>12.2} {:>12} {:<12}",
            case.id,
            case.mode,
            case.median_ms,
            baseline,
            format!("{:?}", case.status).to_lowercase()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    #[test]
    fn discover_cases_returns_lexical_order() -> Result<()> {
        let temp = TempDir::new()?;
        let cases_root = temp.path().join("cases");
        write_case(
            &cases_root,
            "b_case",
            BenchMode::Run,
            r#"{
  "id": "b_case",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "expected_stdout": "2\n",
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;
        write_case(
            &cases_root,
            "a_case",
            BenchMode::Run,
            r#"{
  "id": "a_case",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "expected_stdout": "1\n",
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;

        let cases = discover_cases(&cases_root)?;
        let ids: Vec<_> = cases.iter().map(|case| case.manifest.id.as_str()).collect();
        assert_eq!(ids, vec!["a_case", "b_case"]);
        Ok(())
    }

    #[test]
    fn repo_perf_cases_match_expected_corpus_ids() -> Result<()> {
        let root = workspace_root()?;
        let cases = discover_cases(&root.join("tools").join("perf").join("cases"))?;
        let ids: Vec<_> = cases.into_iter().map(|case| case.manifest.id).collect();
        assert_eq!(
            ids,
            vec![
                "bitmask_checksum_run",
                "bitset_dense_relation_run",
                "cow_collection_chain_run",
                "dp_coin_change_mutable_list_run",
                "grid_bfs_deque_set_run",
                "mutable_bool_dense_relation_run",
                "mutable_map_set_churn_run",
                "mutable_map_sparse_int_probe_run",
                "overload_family_dispatch_run",
                "parse_dense_module_check",
                "wordfreq_map_set_run",
            ]
        );
        Ok(())
    }

    #[test]
    fn cow_collection_chain_case_exercises_shadow_rebinding() -> Result<()> {
        let root = workspace_root()?;
        let source = fs::read_to_string(
            root.join("tools")
                .join("perf")
                .join("cases")
                .join("cow_collection_chain_run")
                .join("main.ky"),
        )?;
        assert!(
            source.contains("let xs = xs.push("),
            "case must use same-name list shadow rebinding"
        );
        assert!(
            source.contains("let m = m.insert(") && source.contains("let m = m.remove("),
            "case must use same-name map shadow rebinding"
        );
        assert!(
            source.contains("let s = s.insert(") && source.contains("let s = s.remove("),
            "case must use same-name set shadow rebinding"
        );
        assert!(
            source.contains("let q = q.push_back("),
            "case must use same-name deque shadow rebinding"
        );
        Ok(())
    }

    #[test]
    fn mutable_bool_dense_relation_case_uses_legacy_bool_table() -> Result<()> {
        let root = workspace_root()?;
        let source = fs::read_to_string(
            root.join("tools")
                .join("perf")
                .join("cases")
                .join("mutable_bool_dense_relation_run")
                .join("main.ky"),
        )?;
        assert!(
            source.contains("MutableList<Bool>"),
            "comparison case must stay on the legacy dense-bool representation"
        );
        assert!(
            source.contains("collections.MutableList.new().push(false)"),
            "comparison case must build a mutable bool table"
        );
        Ok(())
    }

    #[test]
    fn mutable_map_set_churn_case_exercises_mutable_hash_collection_hot_paths() -> Result<()> {
        let root = workspace_root()?;
        let source = fs::read_to_string(
            root.join("tools")
                .join("perf")
                .join("cases")
                .join("mutable_map_set_churn_run")
                .join("main.ky"),
        )?;
        assert!(
            source.contains("MutableMap<Int, Int>"),
            "comparison case must use MutableMap<Int, Int>"
        );
        assert!(
            source.contains("MutableSet<Int>"),
            "comparison case must use MutableSet<Int>"
        );
        assert!(
            source.contains(".insert(")
                && source.contains(".contains(")
                && source.contains(".remove("),
            "comparison case must churn mutable insert/contains/remove operations"
        );
        Ok(())
    }

    #[test]
    fn mutable_map_sparse_probe_case_exercises_with_capacity_and_sparse_int_access() -> Result<()> {
        let root = workspace_root()?;
        let source = fs::read_to_string(
            root.join("tools")
                .join("perf")
                .join("cases")
                .join("mutable_map_sparse_int_probe_run")
                .join("main.ky"),
        )?;
        assert!(
            source.contains("MutableMap<Int, Int>"),
            "sparse probe case must use MutableMap<Int, Int>"
        );
        assert!(
            source.contains("MutableMap.with_capacity"),
            "sparse probe case must exercise mutable map capacity hints"
        );
        assert!(
            source.contains(".get(")
                && source.contains(".contains(")
                && source.contains(".insert(")
                && source.contains(".remove("),
            "sparse probe case must hit the sparse int-key hot path operations"
        );
        Ok(())
    }

    #[test]
    fn overload_family_dispatch_case_exercises_constrained_call_families() -> Result<()> {
        let root = workspace_root()?;
        let source = fs::read_to_string(
            root.join("tools")
                .join("perf")
                .join("cases")
                .join("overload_family_dispatch_run")
                .join("main.ky"),
        )?;
        assert!(
            source.contains("fn score() -> Int")
                && source.contains("fn score(x: Int) -> Int")
                && source.contains("fn score(x: Int, y: Int, z: Int) -> Int"),
            "overload case must define a user function family with arity-distinct members"
        );
        assert!(
            source.contains("fn Counter.bump(self) -> Counter")
                && source.contains("fn Counter.bump(self, delta: Int) -> Counter"),
            "overload case must define a user method family with arity-distinct members"
        );
        assert!(
            source.contains(".starts_with(\"kyo\")")
                && source.contains(".starts_with(\"over\", start: 8)"),
            "overload case must exercise the builtin named-arg call family path"
        );
        Ok(())
    }

    #[test]
    fn discover_cases_rejects_invalid_run_manifest() -> Result<()> {
        let temp = TempDir::new()?;
        let cases_root = temp.path().join("cases");
        write_case(
            &cases_root,
            "broken_case",
            BenchMode::Run,
            r#"{
  "id": "broken_case",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;

        let err = expect_err_text(discover_cases(&cases_root));
        assert!(err.contains("expected_stdout"));
        Ok(())
    }

    #[test]
    fn baseline_roundtrip_preserves_samples_and_fingerprint() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("baseline.json");
        let baseline = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: test_fingerprint("rustc 1.90.0"),
            results: vec![BaselineCaseResult {
                id: "wordfreq".into(),
                mode: BenchMode::Run,
                samples_ms: vec![11.0, 12.0, 13.0],
                median_ms: 12.0,
            }],
        };

        save_baseline(&path, &baseline)?;
        let loaded = load_baseline(&path)?;
        assert_eq!(loaded, baseline);
        Ok(())
    }

    #[test]
    fn compare_against_baseline_passes_small_noise() -> Result<()> {
        let manifests = vec![test_case("wordfreq", BenchMode::Run, 20.0, 15.0)];
        let baseline = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: test_fingerprint("rustc 1.90.0"),
            results: vec![BaselineCaseResult {
                id: "wordfreq".into(),
                mode: BenchMode::Run,
                samples_ms: vec![100.0, 101.0, 99.0],
                median_ms: 100.0,
            }],
        };
        let current = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: baseline.fingerprint.clone(),
            results: vec![BaselineCaseResult {
                id: "wordfreq".into(),
                mode: BenchMode::Run,
                samples_ms: vec![109.0, 110.0, 111.0],
                median_ms: 110.0,
            }],
        };

        let outcome = compare_against_baseline(&manifests, &baseline, &current)?;
        assert_eq!(outcome.cases.len(), 1);
        assert_eq!(outcome.cases[0].status, CaseStatus::Pass);
        Ok(())
    }

    #[test]
    fn compare_against_baseline_fails_when_pct_and_abs_thresholds_breach() -> Result<()> {
        let manifests = vec![test_case("wordfreq", BenchMode::Run, 20.0, 15.0)];
        let baseline = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: test_fingerprint("rustc 1.90.0"),
            results: vec![BaselineCaseResult {
                id: "wordfreq".into(),
                mode: BenchMode::Run,
                samples_ms: vec![100.0, 101.0, 99.0],
                median_ms: 100.0,
            }],
        };
        let current = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: baseline.fingerprint.clone(),
            results: vec![BaselineCaseResult {
                id: "wordfreq".into(),
                mode: BenchMode::Run,
                samples_ms: vec![130.0, 131.0, 132.0],
                median_ms: 131.0,
            }],
        };

        let err = expect_err_text(compare_against_baseline(&manifests, &baseline, &current));
        assert!(err.contains("wordfreq"));
        Ok(())
    }

    #[test]
    fn compare_against_baseline_fails_on_missing_case() -> Result<()> {
        let manifests = vec![test_case("wordfreq", BenchMode::Run, 20.0, 15.0)];
        let baseline = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: test_fingerprint("rustc 1.90.0"),
            results: vec![BaselineCaseResult {
                id: "wordfreq".into(),
                mode: BenchMode::Run,
                samples_ms: vec![100.0, 101.0, 99.0],
                median_ms: 100.0,
            }],
        };
        let current = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: baseline.fingerprint.clone(),
            results: vec![],
        };

        let err = expect_err_text(compare_against_baseline(&manifests, &baseline, &current));
        assert!(err.contains("case set mismatch"));
        Ok(())
    }

    #[test]
    fn record_writes_baseline_and_latest_report_using_fake_binary() -> Result<()> {
        let temp = TempDir::new()?;
        let workspace_root = temp.path().join("workspace");
        fs::create_dir_all(&workspace_root)?;
        let paths = test_paths(&workspace_root)?;

        let binary_path = write_fake_binary(temp.path())?;
        let mut paths = paths;
        paths.binary_path = Some(binary_path);
        paths.fingerprint_override = Some(test_fingerprint("rustc test"));

        write_case_with_entry(
            &paths.cases_root,
            "wordfreq_map_set_run",
            r#"{
  "id": "wordfreq_map_set_run",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "expected_stdout": "123\n",
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;
        fs::write(
            paths
                .cases_root
                .join("wordfreq_map_set_run")
                .join("delay_secs.txt"),
            "0.02\n",
        )?;
        fs::write(
            paths
                .cases_root
                .join("wordfreq_map_set_run")
                .join("run.stdout"),
            "123\n",
        )?;

        let report = record(
            &paths,
            &PerfConfig {
                case_filter: None,
                skip_build: true,
            },
        )?;

        assert!(report.success);
        assert_eq!(report.cases.len(), 1);
        assert!(paths.latest_report_path.exists());
        let baseline_path = expected_baseline_path(&paths, &test_fingerprint("rustc test"));
        assert!(baseline_path.exists());

        let baseline = load_baseline(&baseline_path)?;
        assert_eq!(baseline.results.len(), 1);
        assert_eq!(baseline.results[0].samples_ms.len(), 2);
        assert!(baseline.results[0].median_ms >= 10.0);
        Ok(())
    }

    #[test]
    fn check_passes_for_equal_or_faster_fake_samples() -> Result<()> {
        let temp = TempDir::new()?;
        let workspace_root = temp.path().join("workspace");
        fs::create_dir_all(&workspace_root)?;
        let mut paths = test_paths(&workspace_root)?;
        let fingerprint = test_fingerprint("rustc test");
        paths.fingerprint_override = Some(fingerprint.clone());
        paths.binary_path = Some(write_fake_binary(temp.path())?);

        write_case_with_entry(
            &paths.cases_root,
            "bitmask_checksum_run",
            r#"{
  "id": "bitmask_checksum_run",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "expected_stdout": "77\n",
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;
        let case_dir = paths.cases_root.join("bitmask_checksum_run");
        fs::write(case_dir.join("delay_secs.txt"), "0.01\n")?;
        fs::write(case_dir.join("run.stdout"), "77\n")?;

        let baseline = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: fingerprint.clone(),
            results: vec![BaselineCaseResult {
                id: "bitmask_checksum_run".into(),
                mode: BenchMode::Run,
                samples_ms: vec![50.0, 52.0],
                median_ms: 51.0,
            }],
        };
        let baseline_path = expected_baseline_path(&paths, &fingerprint);
        let baseline_parent =
            baseline_path
                .parent()
                .ok_or_else(|| PerfError::BaselineMismatch {
                    message: "baseline path missing parent".into(),
                })?;
        fs::create_dir_all(baseline_parent)?;
        save_baseline(&baseline_path, &baseline)?;

        let report = check(
            &paths,
            &PerfConfig {
                case_filter: None,
                skip_build: true,
            },
        )?;

        assert!(report.success);
        assert_eq!(report.cases[0].status, CaseStatus::Pass);
        Ok(())
    }

    #[test]
    fn check_fails_for_regression_beyond_threshold() -> Result<()> {
        let temp = TempDir::new()?;
        let workspace_root = temp.path().join("workspace");
        fs::create_dir_all(&workspace_root)?;
        let mut paths = test_paths(&workspace_root)?;
        let fingerprint = test_fingerprint("rustc test");
        paths.fingerprint_override = Some(fingerprint.clone());
        paths.binary_path = Some(write_fake_binary(temp.path())?);

        write_case_with_entry(
            &paths.cases_root,
            "grid_bfs_deque_set_run",
            r#"{
  "id": "grid_bfs_deque_set_run",
  "mode": "run",
  "entry": "main.ky",
  "project": false,
  "expected_stdout": "44\n",
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;
        let case_dir = paths.cases_root.join("grid_bfs_deque_set_run");
        fs::write(case_dir.join("delay_secs.txt"), "0.08\n")?;
        fs::write(case_dir.join("run.stdout"), "44\n")?;

        let baseline = BaselineFile {
            schema_version: SCHEMA_VERSION,
            fingerprint: fingerprint.clone(),
            results: vec![BaselineCaseResult {
                id: "grid_bfs_deque_set_run".into(),
                mode: BenchMode::Run,
                samples_ms: vec![10.0, 12.0],
                median_ms: 11.0,
            }],
        };
        let baseline_path = expected_baseline_path(&paths, &fingerprint);
        let baseline_parent =
            baseline_path
                .parent()
                .ok_or_else(|| PerfError::BaselineMismatch {
                    message: "baseline path missing parent".into(),
                })?;
        fs::create_dir_all(baseline_parent)?;
        save_baseline(&baseline_path, &baseline)?;

        let err = expect_err_text(check(
            &paths,
            &PerfConfig {
                case_filter: None,
                skip_build: true,
            },
        ));

        assert!(err.contains("grid_bfs_deque_set_run"));
        Ok(())
    }

    #[test]
    fn check_fails_with_clear_message_when_matching_baseline_is_missing() -> Result<()> {
        let temp = TempDir::new()?;
        let workspace_root = temp.path().join("workspace");
        fs::create_dir_all(&workspace_root)?;
        let mut paths = test_paths(&workspace_root)?;
        paths.fingerprint_override = Some(test_fingerprint("rustc test"));
        paths.binary_path = Some(write_fake_binary(temp.path())?);

        write_case_with_entry(
            &paths.cases_root,
            "parse_dense_module_check",
            r#"{
  "id": "parse_dense_module_check",
  "mode": "check",
  "entry": "main.ky",
  "project": false,
  "expected_ok": true,
  "warmup_runs": 1,
  "measured_runs": 2,
  "max_regression_pct": 20.0,
  "max_regression_ms": 15.0
}"#,
        )?;
        let case_dir = paths.cases_root.join("parse_dense_module_check");
        fs::write(case_dir.join("check.stdout"), "{ \"diagnostics\": [] }\n")?;

        let err = expect_err_text(check(
            &paths,
            &PerfConfig {
                case_filter: None,
                skip_build: true,
            },
        ));

        assert!(err.contains("missing matching baseline"));
        Ok(())
    }

    fn test_paths(workspace_root: &Path) -> Result<PerfPaths> {
        let paths = PerfPaths::from_workspace_root(workspace_root.to_path_buf());
        fs::create_dir_all(&paths.cases_root)?;
        fs::create_dir_all(&paths.baselines_root)?;
        let latest_parent =
            paths
                .latest_report_path
                .parent()
                .ok_or_else(|| PerfError::BaselineMismatch {
                    message: "latest report path missing parent".into(),
                })?;
        fs::create_dir_all(latest_parent)?;
        Ok(paths)
    }

    fn test_case(
        id: &str,
        mode: BenchMode,
        max_regression_pct: f64,
        max_regression_ms: f64,
    ) -> BenchCase {
        BenchCase {
            dir: PathBuf::from(id),
            manifest: BenchManifest {
                id: id.to_string(),
                mode,
                entry: "main.ky".into(),
                project: false,
                expected_stdout: Some("ok\n".into()),
                expected_ok: Some(true),
                warmup_runs: 1,
                measured_runs: 2,
                max_regression_pct,
                max_regression_ms,
            },
        }
    }

    fn test_fingerprint(rustc: &str) -> EnvironmentFingerprint {
        EnvironmentFingerprint {
            os: "macos".into(),
            arch: "aarch64".into(),
            cpu_model: "Apple Test CPU".into(),
            rustc: rustc.into(),
            profile: PROFILE_NAME.into(),
        }
    }

    fn write_case(cases_root: &Path, id: &str, _mode: BenchMode, manifest: &str) -> Result<()> {
        write_case_with_entry(cases_root, id, manifest)
    }

    fn write_case_with_entry(cases_root: &Path, id: &str, manifest: &str) -> Result<()> {
        let case_dir = cases_root.join(id);
        fs::create_dir_all(&case_dir)?;
        fs::write(case_dir.join("bench.json"), manifest)?;
        fs::write(case_dir.join("main.ky"), "fn main() -> Int { 0 }\n")?;
        Ok(())
    }

    fn write_fake_binary(root: &Path) -> Result<PathBuf> {
        let path = if cfg!(windows) {
            root.join(format!("fake-kyokara-{}.cmd", unique_suffix()))
        } else {
            root.join(format!("fake-kyokara-{}", unique_suffix()))
        };
        if cfg!(windows) {
            fs::write(
                &path,
                "@echo off\r\nsetlocal enabledelayedexpansion\r\nset mode=%1\r\nshift\r\nif \"%mode%\"==\"check\" (\r\n  if \"%1\"==\"--format\" shift\r\n  if \"%1\"==\"json\" shift\r\n)\r\nif \"%1\"==\"--project\" shift\r\nset entry=%1\r\nfor %%I in (\"%entry%\") do set case_dir=%%~dpI\r\nif exist \"%case_dir%delay_secs.txt\" (\r\n  >nul ping -n 2 127.0.0.1\r\n)\r\nif \"%mode%\"==\"run\" (\r\n  if exist \"%case_dir%run.stdout\" type \"%case_dir%run.stdout\"\r\n  exit /b 0\r\n)\r\nif exist \"%case_dir%check.stdout\" type \"%case_dir%check.stdout\"\r\nexit /b 0\r\n",
            )?;
        } else {
            fs::write(
                &path,
                "#!/bin/sh\nmode=\"$1\"\nshift\nif [ \"$mode\" = \"check\" ] && [ \"$1\" = \"--format\" ]; then\n  shift 2\nfi\nif [ \"$1\" = \"--project\" ]; then\n  shift\nfi\nentry=\"$1\"\ncase_dir=$(dirname \"$entry\")\nif [ -f \"$case_dir/delay_secs.txt\" ]; then\n  sleep \"$(tr -d '\\n' < \"$case_dir/delay_secs.txt\")\"\nfi\nif [ \"$mode\" = \"run\" ]; then\n  if [ -f \"$case_dir/run.stdout\" ]; then\n    cat \"$case_dir/run.stdout\"\n  fi\n  exit 0\nfi\nif [ -f \"$case_dir/check.stdout\" ]; then\n  cat \"$case_dir/check.stdout\"\nfi\nexit 0\n",
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&path, perms)?;
            }
        }
        Ok(path)
    }

    fn unique_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    }

    fn expect_err_text<T>(result: Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected error"),
            Err(err) => err.to_string(),
        }
    }
}
