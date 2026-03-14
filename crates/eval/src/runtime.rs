use std::cell::RefCell;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kyokara_runtime::replay::{
    CapabilityCheckEvent, CapabilityOutcome, EffectCallEvent, EffectCallOutcome, EffectOutcomeKind,
    ReplayHeader, ReplayWriter, fingerprint_files,
};
use kyokara_runtime::service::{
    CapabilityCheck, EffectRequest, EffectResponse, RuntimeEffectError, RuntimeService,
};

use crate::error::RuntimeError;
use crate::manifest::CapabilityManifest;

pub(crate) type SharedRuntimeService = Rc<RefCell<Box<dyn RuntimeService>>>;

pub(crate) fn new_live_runtime(
    manifest: Option<CapabilityManifest>,
    replay: Option<ReplayLogConfig>,
) -> Result<SharedRuntimeService, RuntimeError> {
    let runtime = LiveRuntime::new(Box::new(StdHostBackend), manifest, replay)?;
    Ok(Rc::new(RefCell::new(Box::new(runtime))))
}

pub(crate) fn build_replay_header(
    entry_file: &Path,
    project_mode: bool,
    files: impl IntoIterator<Item = PathBuf>,
) -> Result<ReplayHeader, RuntimeError> {
    let entry_file = entry_file
        .canonicalize()
        .map_err(|err| RuntimeError::TypeError(format!("replay log: {err}")))?;
    let program_fingerprint = fingerprint_files(files)
        .map_err(|err| RuntimeError::TypeError(format!("replay log: {err}")))?;
    Ok(ReplayHeader {
        schema_version: 1,
        runtime: "interpreter".to_string(),
        entry_file: entry_file.display().to_string(),
        project_mode,
        program_fingerprint,
    })
}

pub(crate) fn map_runtime_effect_error(err: RuntimeEffectError) -> RuntimeError {
    match err {
        RuntimeEffectError::CapabilityDenied {
            capability,
            required_by_name,
        } => RuntimeError::CapabilityDenied {
            capability,
            function: required_by_name,
        },
        RuntimeEffectError::OperationFailed { operation, message } => {
            RuntimeError::TypeError(format!("{operation}: {message}"))
        }
        RuntimeEffectError::ReplayLog(message) => {
            RuntimeError::TypeError(format!("replay log: {message}"))
        }
    }
}

pub(crate) struct ReplayLogConfig {
    pub path: PathBuf,
    pub header: ReplayHeader,
}

trait HostBackend {
    fn print(&mut self, text: &str, newline: bool) -> Result<(), String>;
    fn read_line(&mut self) -> Result<String, String>;
    fn read_stdin(&mut self) -> Result<String, String>;
    fn read_file(&mut self, path: &str) -> Result<String, String>;
}

struct StdHostBackend;

impl HostBackend for StdHostBackend {
    fn print(&mut self, text: &str, newline: bool) -> Result<(), String> {
        if newline {
            println!("{text}");
        } else {
            print!("{text}");
            std::io::stdout().flush().map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|err| err.to_string())?;
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(line)
    }

    fn read_stdin(&mut self) -> Result<String, String> {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|err| err.to_string())?;
        Ok(buf)
    }

    fn read_file(&mut self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|err| err.to_string())
    }
}

struct LiveRuntime {
    manifest: Option<CapabilityManifest>,
    host: Box<dyn HostBackend>,
    log: Option<ReplayWriter<BufWriter<File>>>,
    next_seq: u64,
}

impl LiveRuntime {
    fn new(
        host: Box<dyn HostBackend>,
        manifest: Option<CapabilityManifest>,
        replay: Option<ReplayLogConfig>,
    ) -> Result<Self, RuntimeError> {
        let mut log = None;
        if let Some(replay) = replay {
            if let Some(parent) = replay.path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| RuntimeError::TypeError(format!("replay log: {err}")))?;
            }
            let file = File::create(&replay.path)
                .map_err(|err| RuntimeError::TypeError(format!("replay log: {err}")))?;
            let mut writer = ReplayWriter::new(BufWriter::new(file));
            writer
                .write_header(&replay.header)
                .map_err(|err| RuntimeError::TypeError(format!("replay log: {err}")))?;
            log = Some(writer);
        }

        Ok(Self {
            manifest,
            host,
            log,
            next_seq: 0,
        })
    }

    fn record_capability_check(
        &mut self,
        check: &CapabilityCheck,
        outcome: CapabilityOutcome,
    ) -> Result<(), RuntimeEffectError> {
        if let Some(log) = &mut self.log {
            let event = CapabilityCheckEvent {
                seq: self.next_seq,
                capability: check.capability.clone(),
                required_by_kind: check.required_by_kind.clone(),
                required_by_name: check.required_by_name.clone(),
                outcome,
            };
            log.write_capability_check(&event)
                .map_err(|err| RuntimeEffectError::ReplayLog(err.to_string()))?;
            self.next_seq += 1;
        }
        Ok(())
    }

    fn record_effect_call(
        &mut self,
        request: &EffectRequest,
        outcome: EffectCallOutcome,
    ) -> Result<(), RuntimeEffectError> {
        if let Some(log) = &mut self.log {
            let event = EffectCallEvent {
                seq: self.next_seq,
                capability: request.capability.clone(),
                operation: request.operation.clone(),
                effect_kind: request.effect_kind,
                required_by_name: request.required_by_name.clone(),
                input: request.input.clone(),
                outcome,
            };
            log.write_effect_call(&event)
                .map_err(|err| RuntimeEffectError::ReplayLog(err.to_string()))?;
            self.next_seq += 1;
        }
        Ok(())
    }
}

impl RuntimeService for LiveRuntime {
    fn authorize(&mut self, check: CapabilityCheck) -> Result<(), RuntimeEffectError> {
        let allowed = self
            .manifest
            .as_ref()
            .is_none_or(|manifest| manifest.is_granted(&check.capability));
        let outcome = if allowed {
            CapabilityOutcome::Allowed
        } else {
            CapabilityOutcome::Denied
        };
        self.record_capability_check(&check, outcome)?;
        if allowed {
            Ok(())
        } else {
            Err(RuntimeEffectError::CapabilityDenied {
                capability: check.capability,
                required_by_name: check.required_by_name,
            })
        }
    }

    fn call(&mut self, request: EffectRequest) -> Result<EffectResponse, RuntimeEffectError> {
        let result = match request.operation.as_str() {
            "print" => {
                let Some(text) = request
                    .input
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                else {
                    return Err(RuntimeEffectError::OperationFailed {
                        operation: "print".to_string(),
                        message: "missing string payload".to_string(),
                    });
                };
                self.host
                    .print(text, false)
                    .map(|()| serde_json::Value::Null)
            }
            "println" => {
                let Some(text) = request
                    .input
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                else {
                    return Err(RuntimeEffectError::OperationFailed {
                        operation: "println".to_string(),
                        message: "missing string payload".to_string(),
                    });
                };
                self.host
                    .print(text, true)
                    .map(|()| serde_json::Value::Null)
            }
            "read_line" => self
                .host
                .read_line()
                .map(|text| serde_json::json!({ "text": text })),
            "read_stdin" => self
                .host
                .read_stdin()
                .map(|text| serde_json::json!({ "text": text })),
            "read_file" => {
                let Some(path) = request
                    .input
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                else {
                    return Err(RuntimeEffectError::OperationFailed {
                        operation: "read_file".to_string(),
                        message: "missing path payload".to_string(),
                    });
                };
                self.host
                    .read_file(path)
                    .map(|text| serde_json::json!({ "text": text }))
            }
            other => {
                return Err(RuntimeEffectError::OperationFailed {
                    operation: other.to_string(),
                    message: "unsupported host effect".to_string(),
                });
            }
        };

        match result {
            Ok(value) => {
                self.record_effect_call(
                    &request,
                    EffectCallOutcome {
                        kind: EffectOutcomeKind::Ok,
                        value: value.clone(),
                        message: None,
                    },
                )?;
                Ok(EffectResponse { value })
            }
            Err(message) => {
                self.record_effect_call(
                    &request,
                    EffectCallOutcome {
                        kind: EffectOutcomeKind::RuntimeError,
                        value: serde_json::Value::Null,
                        message: Some(message.clone()),
                    },
                )?;
                Err(RuntimeEffectError::OperationFailed {
                    operation: request.operation,
                    message,
                })
            }
        }
    }
}
