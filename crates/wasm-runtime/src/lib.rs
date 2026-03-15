//! WASM execution support for Kyokara.

use kyokara_runtime::replay::{EffectKind, RequiredByKind};
use kyokara_runtime::service::{
    CapabilityCheck, EffectRequest, ReplayMode, ReplayRuntime, RuntimeEffectError, RuntimeService,
};
use thiserror::Error;

const HOST_MODULE: &str = "kyokara_host";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HostStatus {
    Ok = 0,
    CapabilityDenied = 1,
    RuntimeError = 2,
    BufferTooSmall = 3,
    BadGuestMemory = 4,
    InvalidUtf8 = 5,
    InvalidRequiredByKind = 6,
}

impl HostStatus {
    fn code(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Error)]
pub enum WasmRuntimeError {
    #[error("invalid WASM module: {0}")]
    InvalidModule(#[source] wasmtime::Error),
    #[error("WASM instantiation failed: {0}")]
    Instantiation(#[source] wasmtime::Error),
    #[error("WASM host linker setup failed: {0}")]
    HostLinker(#[source] wasmtime::Error),
    #[error("missing or invalid export `{export}`: {source}")]
    MissingExport {
        export: &'static str,
        #[source]
        source: wasmtime::Error,
    },
    #[error("WASM `{export}` trapped: {source}")]
    Trap {
        export: &'static str,
        #[source]
        source: wasmtime::Error,
    },
    #[error("WASM guest memory access failed: {0}")]
    GuestMemory(String),
    #[error("WASM replay setup failed: {0}")]
    Replay(#[source] kyokara_runtime::replay::ReplayReadError),
    #[error("WASM runtime finalize failed: {0}")]
    Finalize(#[source] RuntimeEffectError),
}

struct StoreState {
    runtime: Option<Box<dyn RuntimeService>>,
    last_host_error: Option<String>,
}

/// Instantiated WASM program ready to invoke exports.
pub struct WasmProgram {
    store: wasmtime::Store<StoreState>,
    instance: wasmtime::Instance,
}

impl WasmProgram {
    pub fn instantiate(bytes: &[u8]) -> Result<Self, WasmRuntimeError> {
        let engine = wasmtime::Engine::default();
        let wasm_module =
            wasmtime::Module::new(&engine, bytes).map_err(WasmRuntimeError::InvalidModule)?;
        let mut store = wasmtime::Store::new(
            &engine,
            StoreState {
                runtime: None,
                last_host_error: None,
            },
        );
        let instance = wasmtime::Instance::new(&mut store, &wasm_module, &[])
            .map_err(WasmRuntimeError::Instantiation)?;
        Ok(Self { store, instance })
    }

    pub fn instantiate_with_runtime(
        bytes: &[u8],
        runtime: Box<dyn RuntimeService>,
    ) -> Result<Self, WasmRuntimeError> {
        let engine = wasmtime::Engine::default();
        let wasm_module =
            wasmtime::Module::new(&engine, bytes).map_err(WasmRuntimeError::InvalidModule)?;
        let mut store = wasmtime::Store::new(
            &engine,
            StoreState {
                runtime: Some(runtime),
                last_host_error: None,
            },
        );
        let linker = build_host_linker(&engine)?;
        let instance = linker
            .instantiate(&mut store, &wasm_module)
            .map_err(WasmRuntimeError::Instantiation)?;
        Ok(Self { store, instance })
    }

    pub fn instantiate_with_replay_log(
        bytes: &[u8],
        log_path: &std::path::Path,
        mode: ReplayMode,
    ) -> Result<(Self, kyokara_runtime::replay::ReplayHeader), WasmRuntimeError> {
        let (runtime, header) =
            ReplayRuntime::from_log_path(log_path, mode).map_err(WasmRuntimeError::Replay)?;
        Ok((
            Self::instantiate_with_runtime(bytes, Box::new(runtime))?,
            header,
        ))
    }

    pub fn call_main_i64(&mut self) -> Result<i64, WasmRuntimeError> {
        self.call_typed_main_i64("main")
    }

    pub fn call_main_f64(&mut self) -> Result<f64, WasmRuntimeError> {
        self.call_typed_main_f64("main")
    }

    pub fn call_main_i32(&mut self) -> Result<i32, WasmRuntimeError> {
        self.call_typed_main_i32("main")
    }

    pub fn call_export_i32(&mut self, export: &'static str) -> Result<i32, WasmRuntimeError> {
        self.call_typed_main_i32(export)
    }

    pub fn read_memory(&mut self, ptr: u32, len: u32) -> Result<Vec<u8>, WasmRuntimeError> {
        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .ok_or_else(|| WasmRuntimeError::GuestMemory("missing exported memory".to_string()))?;
        let mut buf = vec![0; len as usize];
        memory
            .read(&self.store, ptr as usize, &mut buf)
            .map_err(|err| WasmRuntimeError::GuestMemory(err.to_string()))?;
        Ok(buf)
    }

    pub fn last_host_error(&self) -> Option<&str> {
        self.store.data().last_host_error.as_deref()
    }

    fn call_typed_main_i64(&mut self, export: &'static str) -> Result<i64, WasmRuntimeError> {
        let func = self
            .instance
            .get_typed_func::<(), i64>(&mut self.store, export)
            .map_err(|source| WasmRuntimeError::MissingExport { export, source })?;
        let value = func
            .call(&mut self.store, ())
            .map_err(|source| WasmRuntimeError::Trap { export, source })?;
        self.finalize_runtime()?;
        Ok(value)
    }

    fn call_typed_main_f64(&mut self, export: &'static str) -> Result<f64, WasmRuntimeError> {
        let func = self
            .instance
            .get_typed_func::<(), f64>(&mut self.store, export)
            .map_err(|source| WasmRuntimeError::MissingExport { export, source })?;
        let value = func
            .call(&mut self.store, ())
            .map_err(|source| WasmRuntimeError::Trap { export, source })?;
        self.finalize_runtime()?;
        Ok(value)
    }

    fn call_typed_main_i32(&mut self, export: &'static str) -> Result<i32, WasmRuntimeError> {
        let func = self
            .instance
            .get_typed_func::<(), i32>(&mut self.store, export)
            .map_err(|source| WasmRuntimeError::MissingExport { export, source })?;
        let value = func
            .call(&mut self.store, ())
            .map_err(|source| WasmRuntimeError::Trap { export, source })?;
        self.finalize_runtime()?;
        Ok(value)
    }

    fn finalize_runtime(&mut self) -> Result<(), WasmRuntimeError> {
        if let Some(runtime) = self.store.data_mut().runtime.as_mut() {
            runtime.finalize().map_err(WasmRuntimeError::Finalize)?;
        }
        Ok(())
    }
}

fn build_host_linker(
    engine: &wasmtime::Engine,
) -> Result<wasmtime::Linker<StoreState>, WasmRuntimeError> {
    let mut linker = wasmtime::Linker::new(engine);
    linker
        .func_wrap(
            HOST_MODULE,
            "capability_authorize",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             capability_ptr: i32,
             capability_len: i32,
             required_by_kind: i32,
             required_by_name_ptr: i32,
             required_by_name_len: i32|
             -> i32 {
                let capability =
                    match read_guest_string(&mut caller, capability_ptr, capability_len) {
                        Ok(value) => value,
                        Err(status) => return status.code(),
                    };
                let required_by_name = match read_guest_string(
                    &mut caller,
                    required_by_name_ptr,
                    required_by_name_len,
                ) {
                    Ok(value) => value,
                    Err(status) => return status.code(),
                };
                let required_by_kind = match decode_required_by_kind(required_by_kind) {
                    Ok(value) => value,
                    Err(status) => return status.code(),
                };
                let request = CapabilityCheck {
                    capability,
                    required_by_kind,
                    required_by_name,
                };
                match call_runtime_authorize(&mut caller, request) {
                    Ok(()) => HostStatus::Ok.code(),
                    Err(status) => status.code(),
                }
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "io_print",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             text_ptr: i32,
             text_len: i32,
             required_by_name_ptr: i32,
             required_by_name_len: i32|
             -> i32 {
                call_text_effect(
                    &mut caller,
                    "io",
                    "print",
                    text_ptr,
                    text_len,
                    required_by_name_ptr,
                    required_by_name_len,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "io_println",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             text_ptr: i32,
             text_len: i32,
             required_by_name_ptr: i32,
             required_by_name_len: i32|
             -> i32 {
                call_text_effect(
                    &mut caller,
                    "io",
                    "println",
                    text_ptr,
                    text_len,
                    required_by_name_ptr,
                    required_by_name_len,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "io_read_line",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             buffer_ptr: i32,
             buffer_cap: i32,
             written_len_ptr: i32,
             required_by_name_ptr: i32,
             required_by_name_len: i32|
             -> i32 {
                call_text_read_effect(
                    &mut caller,
                    "io",
                    "read_line",
                    serde_json::json!({}),
                    buffer_ptr,
                    buffer_cap,
                    written_len_ptr,
                    required_by_name_ptr,
                    required_by_name_len,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "io_read_stdin",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             buffer_ptr: i32,
             buffer_cap: i32,
             written_len_ptr: i32,
             required_by_name_ptr: i32,
             required_by_name_len: i32|
             -> i32 {
                call_text_read_effect(
                    &mut caller,
                    "io",
                    "read_stdin",
                    serde_json::json!({}),
                    buffer_ptr,
                    buffer_cap,
                    written_len_ptr,
                    required_by_name_ptr,
                    required_by_name_len,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "fs_read_file",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             path_ptr: i32,
             path_len: i32,
             buffer_ptr: i32,
             buffer_cap: i32,
             written_len_ptr: i32,
             required_by_name_ptr: i32,
             required_by_name_len: i32|
             -> i32 {
                let path = match read_guest_string(&mut caller, path_ptr, path_len) {
                    Ok(value) => value,
                    Err(status) => return status.code(),
                };
                call_text_read_effect(
                    &mut caller,
                    "fs",
                    "read_file",
                    serde_json::json!({ "path": path }),
                    buffer_ptr,
                    buffer_cap,
                    written_len_ptr,
                    required_by_name_ptr,
                    required_by_name_len,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    Ok(linker)
}

fn decode_required_by_kind(raw: i32) -> Result<RequiredByKind, HostStatus> {
    match raw {
        0 => Ok(RequiredByKind::Builtin),
        1 => Ok(RequiredByKind::UserFn),
        _ => Err(HostStatus::InvalidRequiredByKind),
    }
}

fn call_text_effect(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    capability: &str,
    operation: &str,
    text_ptr: i32,
    text_len: i32,
    required_by_name_ptr: i32,
    required_by_name_len: i32,
) -> i32 {
    let text = match read_guest_string(caller, text_ptr, text_len) {
        Ok(value) => value,
        Err(status) => return status.code(),
    };
    let required_by_name =
        match read_guest_string(caller, required_by_name_ptr, required_by_name_len) {
            Ok(value) => value,
            Err(status) => return status.code(),
        };
    let request = EffectRequest {
        capability: capability.to_string(),
        operation: operation.to_string(),
        effect_kind: EffectKind::Write,
        required_by_name,
        input: serde_json::json!({ "text": text }),
    };
    match call_runtime_effect(caller, request) {
        Ok(_) => HostStatus::Ok.code(),
        Err(status) => status.code(),
    }
}

fn call_text_read_effect(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    capability: &str,
    operation: &str,
    input: serde_json::Value,
    buffer_ptr: i32,
    buffer_cap: i32,
    written_len_ptr: i32,
    required_by_name_ptr: i32,
    required_by_name_len: i32,
) -> i32 {
    let required_by_name =
        match read_guest_string(caller, required_by_name_ptr, required_by_name_len) {
            Ok(value) => value,
            Err(status) => return status.code(),
        };
    let request = EffectRequest {
        capability: capability.to_string(),
        operation: operation.to_string(),
        effect_kind: EffectKind::Read,
        required_by_name,
        input,
    };
    let response = match call_runtime_effect(caller, request) {
        Ok(value) => value,
        Err(status) => return status.code(),
    };
    let Some(text) = response
        .value
        .get("text")
        .and_then(serde_json::Value::as_str)
    else {
        store_host_error(
            caller,
            format!("{operation}: missing text response payload"),
        );
        return HostStatus::RuntimeError.code();
    };
    if text.len() > buffer_cap.max(0) as usize {
        return HostStatus::BufferTooSmall.code();
    }
    if write_guest_bytes(caller, buffer_ptr, text.as_bytes()).is_err() {
        return HostStatus::BadGuestMemory.code();
    }
    if write_guest_i32(caller, written_len_ptr, text.len() as i32).is_err() {
        return HostStatus::BadGuestMemory.code();
    }
    HostStatus::Ok.code()
}

fn call_runtime_authorize(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    request: CapabilityCheck,
) -> Result<(), HostStatus> {
    let result = {
        let data = caller.data_mut();
        let Some(runtime) = data.runtime.as_mut() else {
            data.last_host_error = Some("missing runtime service".to_string());
            return Err(HostStatus::RuntimeError);
        };
        runtime.authorize(request)
    };
    match result {
        Ok(()) => Ok(()),
        Err(RuntimeEffectError::CapabilityDenied {
            required_by_name, ..
        }) => {
            store_host_error(
                caller,
                format!("capability denied for `{required_by_name}`"),
            );
            Err(HostStatus::CapabilityDenied)
        }
        Err(err) => {
            store_host_error(caller, err.to_string());
            Err(HostStatus::RuntimeError)
        }
    }
}

fn call_runtime_effect(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    request: EffectRequest,
) -> Result<kyokara_runtime::service::EffectResponse, HostStatus> {
    let result = {
        let data = caller.data_mut();
        let Some(runtime) = data.runtime.as_mut() else {
            data.last_host_error = Some("missing runtime service".to_string());
            return Err(HostStatus::RuntimeError);
        };
        runtime.call(request)
    };
    match result {
        Ok(response) => Ok(response),
        Err(RuntimeEffectError::CapabilityDenied {
            required_by_name, ..
        }) => {
            store_host_error(
                caller,
                format!("capability denied for `{required_by_name}`"),
            );
            Err(HostStatus::CapabilityDenied)
        }
        Err(err) => {
            store_host_error(caller, err.to_string());
            Err(HostStatus::RuntimeError)
        }
    }
}

fn read_guest_string(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    len: i32,
) -> Result<String, HostStatus> {
    let bytes = read_guest_bytes(caller, ptr, len)?;
    String::from_utf8(bytes).map_err(|_| HostStatus::InvalidUtf8)
}

fn read_guest_bytes(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, HostStatus> {
    if ptr < 0 || len < 0 {
        return Err(HostStatus::BadGuestMemory);
    }
    let Some(memory) = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
    else {
        return Err(HostStatus::BadGuestMemory);
    };
    let mut buf = vec![0; len as usize];
    memory
        .read(caller, ptr as usize, &mut buf)
        .map_err(|_| HostStatus::BadGuestMemory)?;
    Ok(buf)
}

fn write_guest_bytes(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    bytes: &[u8],
) -> Result<(), HostStatus> {
    if ptr < 0 {
        return Err(HostStatus::BadGuestMemory);
    }
    let Some(memory) = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
    else {
        return Err(HostStatus::BadGuestMemory);
    };
    memory
        .write(caller, ptr as usize, bytes)
        .map_err(|_| HostStatus::BadGuestMemory)
}

fn write_guest_i32(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    value: i32,
) -> Result<(), HostStatus> {
    write_guest_bytes(caller, ptr, &value.to_le_bytes())
}

fn store_host_error(caller: &mut wasmtime::Caller<'_, StoreState>, message: String) {
    caller.data_mut().last_host_error = Some(message);
}
