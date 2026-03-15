use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kyokara_runtime::replay::{
    INTERPRETER_RUNTIME, ReplayHeader, ReplayLogConfig, ReplayReadError,
};
use kyokara_runtime::service::{
    LiveRuntime, ReplayMode, ReplayRuntime, RuntimeEffectError, RuntimeService, StdHostBackend,
    build_replay_header as shared_build_replay_header,
};

use crate::error::RuntimeError;
use crate::manifest::CapabilityManifest;

pub(crate) type SharedRuntimeService = Rc<RefCell<Box<dyn RuntimeService>>>;

pub(crate) fn new_live_runtime(
    manifest: Option<CapabilityManifest>,
    replay: Option<ReplayLogConfig>,
) -> Result<SharedRuntimeService, RuntimeError> {
    let runtime = LiveRuntime::new(
        Box::new(StdHostBackend),
        Box::new(move |capability| {
            manifest
                .as_ref()
                .is_none_or(|caps| caps.is_granted(capability))
        }),
        replay,
    )
    .map_err(map_runtime_effect_error)?;
    Ok(Rc::new(RefCell::new(Box::new(runtime))))
}

pub(crate) fn new_replay_runtime(
    log_path: &Path,
    mode: ReplayMode,
) -> Result<(SharedRuntimeService, ReplayHeader), RuntimeError> {
    let (runtime, header) =
        ReplayRuntime::from_log_path(log_path, mode).map_err(map_replay_read_error)?;
    if header.runtime != INTERPRETER_RUNTIME {
        return Err(RuntimeError::TypeError(format!(
            "replay log: unsupported replay runtime `{}`",
            header.runtime
        )));
    }
    Ok((Rc::new(RefCell::new(Box::new(runtime))), header))
}

pub(crate) fn build_replay_header(
    entry_file: &Path,
    project_mode: bool,
    files: impl IntoIterator<Item = PathBuf>,
) -> Result<ReplayHeader, RuntimeError> {
    shared_build_replay_header(entry_file, project_mode, files, INTERPRETER_RUNTIME)
        .map_err(|err| RuntimeError::TypeError(format!("replay log: {err}")))
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

fn map_replay_read_error(err: ReplayReadError) -> RuntimeError {
    RuntimeError::TypeError(format!("replay log: {err}"))
}
