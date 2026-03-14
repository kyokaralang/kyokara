use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredByKind {
    Builtin,
    UserFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityOutcome {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectOutcomeKind {
    Ok,
    CapabilityDenied,
    RuntimeError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramFingerprintEntry {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayHeader {
    pub schema_version: u32,
    pub runtime: String,
    pub entry_file: String,
    pub project_mode: bool,
    pub program_fingerprint: Vec<ProgramFingerprintEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCheckEvent {
    pub seq: u64,
    pub capability: String,
    pub required_by_kind: RequiredByKind,
    pub required_by_name: String,
    pub outcome: CapabilityOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectCallOutcome {
    pub kind: EffectOutcomeKind,
    pub value: serde_json::Value,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectCallEvent {
    pub seq: u64,
    pub capability: String,
    pub operation: String,
    pub effect_kind: EffectKind,
    pub required_by_name: String,
    pub input: serde_json::Value,
    pub outcome: EffectCallOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayLogLine {
    Header(ReplayHeader),
    CapabilityCheck(CapabilityCheckEvent),
    EffectCall(EffectCallEvent),
}

pub struct ReplayWriter<W: Write> {
    writer: W,
}

impl<W: Write> ReplayWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn write_header(&mut self, header: &ReplayHeader) -> io::Result<()> {
        self.write_line(&ReplayLogLine::Header(header.clone()))
    }

    pub fn write_capability_check(&mut self, event: &CapabilityCheckEvent) -> io::Result<()> {
        self.write_line(&ReplayLogLine::CapabilityCheck(event.clone()))
    }

    pub fn write_effect_call(&mut self, event: &EffectCallEvent) -> io::Result<()> {
        self.write_line(&ReplayLogLine::EffectCall(event.clone()))
    }

    pub fn into_inner(self) -> W {
        self.writer
    }

    fn write_line(&mut self, line: &ReplayLogLine) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, line).map_err(io::Error::other)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

pub fn fingerprint_files(
    files: impl IntoIterator<Item = PathBuf>,
) -> io::Result<Vec<ProgramFingerprintEntry>> {
    let mut entries = Vec::new();
    for file in files {
        entries.push(fingerprint_file(&file)?);
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

pub fn fingerprint_file(path: &Path) -> io::Result<ProgramFingerprintEntry> {
    let abs = path.canonicalize()?;
    let bytes = std::fs::read(&abs)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(ProgramFingerprintEntry {
        path: abs.display().to_string(),
        sha256: format!("{:x}", hasher.finalize()),
    })
}
