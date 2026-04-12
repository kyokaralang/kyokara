//! WASM execution support for Kyokara.

use kyokara_runtime::replay::{EffectKind, RequiredByKind};
use kyokara_runtime::service::{
    CapabilityCheck, EffectRequest, ReplayMode, ReplayRuntime, RuntimeEffectError, RuntimeService,
};
use md5::Digest;
use once_cell::sync::OnceCell;
use thiserror::Error;

const HOST_MODULE: &str = "kyokara_host";
const MAX_WASM_STACK_BYTES: usize = 64 * 1024 * 1024;
const STRING_SPECIAL_TAG_MASK: i32 = i32::MIN;
const STRING_FORWARD_SENTINEL: i32 = -1;
const STRING_MD5_SENTINEL: i32 = -2;
const STRING_SLICE_SENTINEL: i32 = -3;
const MD5_INLINE_DIGEST_SOURCE_PTR: i32 = -1;
const MD5_DIGEST_OFFSET: i32 = 16;
const MD5_DIGEST_SIZE: i32 = 16;
const MD5_SPECIAL_STRING_SIZE: i32 = 32;

static WASM_ENGINE: OnceCell<wasmtime::Engine> = OnceCell::new();
static HOST_LINKER: OnceCell<wasmtime::Linker<StoreState>> = OnceCell::new();

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
enum ParseHelperStatus {
    Ok = 0,
    Err = 1,
}

impl HostStatus {
    fn code(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Error)]
pub enum WasmRuntimeError {
    #[error("WASM engine setup failed: {0}")]
    Engine(#[source] wasmtime::Error),
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
    last_md5_digest: Option<(i32, [u8; 16])>,
    guest_memory: Option<wasmtime::Memory>,
    guest_alloc: Option<wasmtime::TypedFunc<i32, i32>>,
}

/// Instantiated WASM program ready to invoke exports.
pub struct WasmProgram {
    store: wasmtime::Store<StoreState>,
    instance: wasmtime::Instance,
}

impl WasmProgram {
    pub fn instantiate(bytes: &[u8]) -> Result<Self, WasmRuntimeError> {
        let engine = shared_engine()?;
        let wasm_module =
            wasmtime::Module::new(engine, bytes).map_err(WasmRuntimeError::InvalidModule)?;
        let mut store = wasmtime::Store::new(
            engine,
            StoreState {
                runtime: None,
                last_host_error: None,
                last_md5_digest: None,
                guest_memory: None,
                guest_alloc: None,
            },
        );
        let instance = shared_host_linker()?
            .instantiate(&mut store, &wasm_module)
            .map_err(WasmRuntimeError::Instantiation)?;
        cache_guest_exports(&mut store, &instance)?;
        Ok(Self { store, instance })
    }

    pub fn instantiate_with_runtime(
        bytes: &[u8],
        runtime: Box<dyn RuntimeService>,
    ) -> Result<Self, WasmRuntimeError> {
        let engine = shared_engine()?;
        let wasm_module =
            wasmtime::Module::new(engine, bytes).map_err(WasmRuntimeError::InvalidModule)?;
        let mut store = wasmtime::Store::new(
            engine,
            StoreState {
                runtime: Some(runtime),
                last_host_error: None,
                last_md5_digest: None,
                guest_memory: None,
                guest_alloc: None,
            },
        );
        let instance = shared_host_linker()?
            .instantiate(&mut store, &wasm_module)
            .map_err(WasmRuntimeError::Instantiation)?;
        cache_guest_exports(&mut store, &instance)?;
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

    pub fn read_string(&mut self, ptr: u32) -> Result<String, WasmRuntimeError> {
        read_program_string(self, ptr)
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

fn cache_guest_exports(
    store: &mut wasmtime::Store<StoreState>,
    instance: &wasmtime::Instance,
) -> Result<(), WasmRuntimeError> {
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| WasmRuntimeError::GuestMemory("missing exported memory".to_string()))?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut *store, "__kyokara_alloc")
        .map_err(|source| WasmRuntimeError::MissingExport {
            export: "__kyokara_alloc",
            source,
        })?;
    let data = store.data_mut();
    data.guest_memory = Some(memory);
    data.guest_alloc = Some(alloc);
    Ok(())
}

fn build_engine() -> Result<wasmtime::Engine, WasmRuntimeError> {
    let mut config = wasmtime::Config::new();
    config.max_wasm_stack(MAX_WASM_STACK_BYTES);
    config.cranelift_opt_level(wasmtime::OptLevel::SpeedAndSize);
    wasmtime::Engine::new(&config).map_err(WasmRuntimeError::Engine)
}

fn shared_engine() -> Result<&'static wasmtime::Engine, WasmRuntimeError> {
    WASM_ENGINE.get_or_try_init(build_engine)
}

fn shared_host_linker() -> Result<&'static wasmtime::Linker<StoreState>, WasmRuntimeError> {
    HOST_LINKER.get_or_try_init(|| build_host_linker(shared_engine()?))
}

fn build_host_linker(
    engine: &wasmtime::Engine,
) -> Result<wasmtime::Linker<StoreState>, WasmRuntimeError> {
    let mut linker = wasmtime::Linker::new(engine);
    linker
        .func_wrap(
            HOST_MODULE,
            "string_to_upper",
            |mut caller: wasmtime::Caller<'_, StoreState>, text_ptr: i32, text_len: i32| -> i32 {
                call_string_host_helper(&mut caller, text_ptr, text_len, |text| text.to_uppercase())
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_to_lower",
            |mut caller: wasmtime::Caller<'_, StoreState>, text_ptr: i32, text_len: i32| -> i32 {
                call_string_host_helper(&mut caller, text_ptr, text_len, |text| text.to_lowercase())
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_md5",
            |mut caller: wasmtime::Caller<'_, StoreState>, text_object_ptr: i32| -> i32 {
                call_string_md5_host_helper(&mut caller, text_object_ptr)
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_md5_concat_int",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             text_object_ptr: i32,
             suffix: i64|
             -> i32 {
                call_string_md5_concat_int_host_helper(&mut caller, text_object_ptr, suffix)
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_md5_materialize",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             text_object_ptr: i32,
             dst_ptr: i32|
             -> i32 {
                call_string_md5_materialize(&mut caller, text_object_ptr, dst_ptr)
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_md5_char_code",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             md5_object_ptr: i32,
             index: i32|
             -> i32 { call_string_md5_char_code(&mut caller, md5_object_ptr, index) },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_md5_contains",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             md5_object_ptr: i32,
             needle_object_ptr: i32|
             -> i32 { call_string_md5_contains(&mut caller, md5_object_ptr, needle_object_ptr) },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "string_md5_starts_with",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             md5_object_ptr: i32,
             prefix_object_ptr: i32|
             -> i32 {
                call_string_md5_starts_with(&mut caller, md5_object_ptr, prefix_object_ptr)
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "parse_int",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             text_ptr: i32,
             text_len: i32,
             value_out_ptr: i32,
             message_out_ptr: i32|
             -> i32 {
                call_parse_int_helper(
                    &mut caller,
                    text_ptr,
                    text_len,
                    value_out_ptr,
                    message_out_ptr,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
    linker
        .func_wrap(
            HOST_MODULE,
            "parse_float",
            |mut caller: wasmtime::Caller<'_, StoreState>,
             text_ptr: i32,
             text_len: i32,
             value_out_ptr: i32,
             message_out_ptr: i32|
             -> i32 {
                call_parse_float_helper(
                    &mut caller,
                    text_ptr,
                    text_len,
                    value_out_ptr,
                    message_out_ptr,
                )
            },
        )
        .map_err(WasmRuntimeError::HostLinker)?;
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

fn call_string_host_helper(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    text_ptr: i32,
    text_len: i32,
    transform: impl FnOnce(String) -> String,
) -> i32 {
    let text = match read_guest_string(caller, text_ptr, text_len) {
        Ok(value) => value,
        Err(status) => return status.code(),
    };
    match alloc_guest_string(caller, &transform(text)) {
        Ok(ptr) => ptr,
        Err(status) => status.code(),
    }
}

fn call_string_md5_host_helper(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    text_object_ptr: i32,
) -> i32 {
    match alloc_guest_md5_string(caller, text_object_ptr) {
        Ok(ptr) => ptr,
        Err(status) => status.code(),
    }
}

fn call_string_md5_materialize(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
    dst_ptr: i32,
) -> i32 {
    let digest = match read_or_compute_md5_digest(caller, md5_object_ptr) {
        Ok(digest) => digest,
        Err(status) => return status.code(),
    };
    let digest_bytes = md5_hex_bytes(digest);
    match write_guest_bytes(caller, dst_ptr, &digest_bytes) {
        Ok(()) => HostStatus::Ok.code(),
        Err(status) => status.code(),
    }
}

fn call_string_md5_char_code(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
    index: i32,
) -> i32 {
    if !(0..32).contains(&index) {
        return HostStatus::BadGuestMemory.code();
    }

    let digest = match read_or_compute_md5_digest(caller, md5_object_ptr) {
        Ok(digest) => digest,
        Err(status) => return status.code(),
    };
    md5_hex_char_code(digest, index as usize) as i32
}

fn call_string_md5_contains(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
    needle_object_ptr: i32,
) -> i32 {
    let header = match read_guest_header(caller, needle_object_ptr) {
        Ok(header) => header,
        Err(_status) => return 0,
    };
    let raw_len = match header[0..4].try_into() {
        Ok(bytes) => i32::from_le_bytes(bytes),
        Err(_) => return 0,
    };
    if raw_len < 0 {
        return 0;
    }
    if raw_len == 0 {
        return 1;
    }
    if raw_len > 32 {
        return 0;
    }

    let needle_ptr = match needle_object_ptr.checked_add(8) {
        Some(ptr) => ptr,
        None => return 0,
    };
    let needle = match read_guest_slice(caller, needle_ptr, raw_len) {
        Ok(needle) => needle.to_vec(),
        Err(_status) => return 0,
    };
    let digest = match read_or_compute_md5_digest(caller, md5_object_ptr) {
        Ok(digest) => digest,
        Err(_status) => return 0,
    };

    let max_start = 32usize.saturating_sub(needle.len());
    let mut start = 0usize;
    while start <= max_start {
        let mut index = 0usize;
        while index < needle.len() {
            if md5_hex_char_code(digest, start + index) != needle[index] {
                break;
            }
            index += 1;
        }
        if index == needle.len() {
            return 1;
        }
        start += 1;
    }
    0
}

fn call_string_md5_starts_with(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
    prefix_object_ptr: i32,
) -> i32 {
    let header = match read_guest_header(caller, prefix_object_ptr) {
        Ok(header) => header,
        Err(_status) => return 0,
    };
    let raw_len = match header[0..4].try_into() {
        Ok(bytes) => i32::from_le_bytes(bytes),
        Err(_) => return 0,
    };
    if raw_len < 0 {
        return 0;
    }
    if raw_len > 32 {
        return 0;
    }

    let prefix_ptr = match prefix_object_ptr.checked_add(8) {
        Some(ptr) => ptr,
        None => return 0,
    };
    let prefix = match read_guest_slice(caller, prefix_ptr, raw_len) {
        Ok(prefix) => prefix.to_vec(),
        Err(_status) => return 0,
    };
    let digest = match read_or_compute_md5_digest(caller, md5_object_ptr) {
        Ok(digest) => digest,
        Err(_status) => return 0,
    };

    let mut index = 0usize;
    while index < prefix.len() {
        if md5_hex_char_code(digest, index) != prefix[index] {
            return 0;
        }
        index += 1;
    }
    1
}

fn call_parse_int_helper(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    text_ptr: i32,
    text_len: i32,
    value_out_ptr: i32,
    message_out_ptr: i32,
) -> i32 {
    let text = match read_guest_string(caller, text_ptr, text_len) {
        Ok(value) => value,
        Err(status) => return status.code(),
    };
    match text.parse::<i64>() {
        Ok(value) => {
            if write_guest_i64(caller, value_out_ptr, value).is_err() {
                return HostStatus::BadGuestMemory.code();
            }
            ParseHelperStatus::Ok as i32
        }
        Err(err) => {
            let message_ptr = match alloc_guest_string(caller, &err.to_string()) {
                Ok(ptr) => ptr,
                Err(status) => return status.code(),
            };
            if write_guest_i32(caller, message_out_ptr, message_ptr).is_err() {
                return HostStatus::BadGuestMemory.code();
            }
            ParseHelperStatus::Err as i32
        }
    }
}

fn call_string_md5_concat_int_host_helper(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    text_object_ptr: i32,
    suffix: i64,
) -> i32 {
    let mut hasher = md5::Md5::new();
    match update_md5_from_guest_string_object(caller, text_object_ptr, &mut hasher) {
        Ok(()) => {
            update_md5_from_int_decimal(&mut hasher, suffix);
            let digest: [u8; 16] = hasher.finalize().into();
            match alloc_guest_cached_md5_string(caller, digest) {
                Ok(ptr) => ptr,
                Err(status) => {
                    store_host_error(
                        caller,
                        format!("WASM string md5 concat int failed: {status:?}"),
                    );
                    0
                }
            }
        }
        Err(status) => {
            store_host_error(
                caller,
                format!("WASM string md5 concat int failed: {status:?}"),
            );
            0
        }
    }
}

fn call_parse_float_helper(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    text_ptr: i32,
    text_len: i32,
    value_out_ptr: i32,
    message_out_ptr: i32,
) -> i32 {
    let text = match read_guest_string(caller, text_ptr, text_len) {
        Ok(value) => value,
        Err(status) => return status.code(),
    };
    match text.parse::<f64>() {
        Ok(value) => {
            if write_guest_f64(caller, value_out_ptr, value).is_err() {
                return HostStatus::BadGuestMemory.code();
            }
            ParseHelperStatus::Ok as i32
        }
        Err(err) => {
            let message_ptr = match alloc_guest_string(caller, &err.to_string()) {
                Ok(ptr) => ptr,
                Err(status) => return status.code(),
            };
            if write_guest_i32(caller, message_out_ptr, message_ptr).is_err() {
                return HostStatus::BadGuestMemory.code();
            }
            ParseHelperStatus::Err as i32
        }
    }
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
    let bytes = read_guest_slice(caller, ptr, len)?;
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| HostStatus::InvalidUtf8)
}

fn lookup_cached_md5_digest(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
) -> Option<[u8; 16]> {
    if let Some((ptr, digest)) = caller.data().last_md5_digest
        && ptr == md5_object_ptr
    {
        return Some(digest);
    }
    let header = read_guest_header(caller, md5_object_ptr).ok()?;
    let raw_len = i32::from_le_bytes(header[0..4].try_into().ok()?);
    if raw_len >= 0 {
        return None;
    }
    let source_ptr = i32::from_le_bytes(header[8..12].try_into().ok()?);
    let rhs_or_sentinel = i32::from_le_bytes(header[12..16].try_into().ok()?);
    if source_ptr != MD5_INLINE_DIGEST_SOURCE_PTR || rhs_or_sentinel != STRING_MD5_SENTINEL {
        return None;
    }
    let digest_bytes = read_guest_slice(caller, md5_object_ptr + MD5_DIGEST_OFFSET, MD5_DIGEST_SIZE)
        .ok()?;
    let mut digest = [0_u8; 16];
    digest.copy_from_slice(digest_bytes);
    caller.data_mut().last_md5_digest = Some((md5_object_ptr, digest));
    Some(digest)
}

fn store_cached_md5_digest(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
    digest: [u8; 16],
) {
    caller.data_mut().last_md5_digest = Some((md5_object_ptr, digest));
    let _ = write_guest_i32(caller, md5_object_ptr + 8, MD5_INLINE_DIGEST_SOURCE_PTR);
    let _ = write_guest_bytes(caller, md5_object_ptr + MD5_DIGEST_OFFSET, &digest);
}

fn read_or_compute_md5_digest(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    md5_object_ptr: i32,
) -> Result<[u8; 16], HostStatus> {
    if let Some(digest) = lookup_cached_md5_digest(caller, md5_object_ptr) {
        return Ok(digest);
    }

    let mut remaining_rounds = 0usize;
    let mut current_ptr = md5_object_ptr;
    loop {
        if let Some(digest) = lookup_cached_md5_digest(caller, current_ptr) {
            let digest = apply_md5_hex_rounds(digest, remaining_rounds);
            store_cached_md5_digest(caller, md5_object_ptr, digest);
            return Ok(digest);
        }

        let header = read_guest_header(caller, current_ptr)?;
        let raw_len = i32::from_le_bytes(
            header[0..4]
                .try_into()
                .map_err(|_| HostStatus::BadGuestMemory)?,
        );
        if raw_len >= 0 {
            break;
        }

        let source_ptr = i32::from_le_bytes(
            header[8..12]
                .try_into()
                .map_err(|_| HostStatus::BadGuestMemory)?,
        );
        let rhs_or_sentinel = i32::from_le_bytes(
            header[12..16]
                .try_into()
                .map_err(|_| HostStatus::BadGuestMemory)?,
        );
        if rhs_or_sentinel != STRING_MD5_SENTINEL {
            return Err(HostStatus::BadGuestMemory);
        }
        if source_ptr == MD5_INLINE_DIGEST_SOURCE_PTR {
            let digest = lookup_cached_md5_digest(caller, current_ptr)
                .ok_or(HostStatus::BadGuestMemory)?;
            let digest = apply_md5_hex_rounds(digest, remaining_rounds);
            store_cached_md5_digest(caller, md5_object_ptr, digest);
            return Ok(digest);
        }
        if source_ptr < 0 {
            return Err(HostStatus::BadGuestMemory);
        }

        remaining_rounds += 1;
        current_ptr = source_ptr;
    }

    if remaining_rounds == 0 {
        return Err(HostStatus::BadGuestMemory);
    }

    let digest = compute_guest_string_object_md5_digest(caller, current_ptr)?;
    let digest = apply_md5_hex_rounds(digest, remaining_rounds.saturating_sub(1));
    store_cached_md5_digest(caller, md5_object_ptr, digest);
    Ok(digest)
}

fn compute_guest_string_object_md5_digest(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
) -> Result<[u8; 16], HostStatus> {
    let mut hasher = md5::Md5::new();
    update_md5_from_guest_string_object(caller, ptr, &mut hasher)?;
    Ok(hasher.finalize().into())
}

fn update_md5_from_guest_string_object(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    hasher: &mut md5::Md5,
) -> Result<(), HostStatus> {
    const INLINE_STACK_CAPACITY: usize = 8;

    let mut inline_stack = [0_i32; INLINE_STACK_CAPACITY];
    let mut inline_len = 1usize;
    let mut overflow_stack: Option<Vec<i32>> = None;
    inline_stack[0] = ptr;

    while let Some(current_ptr) =
        pop_i32_stack(&mut inline_stack, &mut inline_len, &mut overflow_stack)
    {
        let header = read_guest_header(caller, current_ptr)?;
        let raw_len = i32::from_le_bytes(
            header[0..4]
                .try_into()
                .map_err(|_| HostStatus::BadGuestMemory)?,
        );
        if raw_len >= 0 {
            let text_ptr = current_ptr
                .checked_add(8)
                .ok_or(HostStatus::BadGuestMemory)?;
            let bytes = read_guest_slice(caller, text_ptr, raw_len)?;
            hasher.update(bytes);
            continue;
        }

        let source_ptr = i32::from_le_bytes(
            header[8..12]
                .try_into()
                .map_err(|_| HostStatus::BadGuestMemory)?,
        );
        let rhs_or_sentinel = i32::from_le_bytes(
            header[12..16]
                .try_into()
                .map_err(|_| HostStatus::BadGuestMemory)?,
        );
        if source_ptr < 0 {
            return Err(HostStatus::BadGuestMemory);
        }
        if rhs_or_sentinel == STRING_FORWARD_SENTINEL {
            push_i32_stack(
                &mut inline_stack,
                &mut inline_len,
                &mut overflow_stack,
                source_ptr,
            );
            continue;
        }
        if rhs_or_sentinel == STRING_MD5_SENTINEL {
            let digest = read_or_compute_md5_digest(caller, current_ptr)?;
            let hex_bytes = md5_hex_bytes(digest);
            hasher.update(&hex_bytes);
            continue;
        }
        if rhs_or_sentinel < 0 {
            return Err(HostStatus::BadGuestMemory);
        }

        push_i32_stack(
            &mut inline_stack,
            &mut inline_len,
            &mut overflow_stack,
            rhs_or_sentinel,
        );
        push_i32_stack(
            &mut inline_stack,
            &mut inline_len,
            &mut overflow_stack,
            source_ptr,
        );
    }
    Ok(())
}

fn update_md5_from_int_decimal(hasher: &mut md5::Md5, value: i64) {
    let negative = value < 0;
    let mut magnitude = value.unsigned_abs();
    let mut buf = [0_u8; 20];
    let mut index = buf.len();

    loop {
        index -= 1;
        buf[index] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }

    if negative {
        index -= 1;
        buf[index] = b'-';
    }

    hasher.update(&buf[index..]);
}

fn pop_i32_stack(
    inline_stack: &mut [i32; 8],
    inline_len: &mut usize,
    overflow_stack: &mut Option<Vec<i32>>,
) -> Option<i32> {
    if let Some(stack) = overflow_stack.as_mut() {
        return stack.pop();
    }

    if *inline_len == 0 {
        None
    } else {
        *inline_len -= 1;
        Some(inline_stack[*inline_len])
    }
}

fn push_i32_stack(
    inline_stack: &mut [i32; 8],
    inline_len: &mut usize,
    overflow_stack: &mut Option<Vec<i32>>,
    value: i32,
) {
    if let Some(stack) = overflow_stack.as_mut() {
        stack.push(value);
        return;
    }

    if *inline_len < inline_stack.len() {
        inline_stack[*inline_len] = value;
        *inline_len += 1;
        return;
    }

    let mut stack = Vec::with_capacity(inline_stack.len() * 2);
    stack.extend_from_slice(&inline_stack[..*inline_len]);
    stack.push(value);
    *overflow_stack = Some(stack);
}

fn md5_hex_char_code(digest: [u8; 16], index: usize) -> u8 {
    let byte = digest[index / 2];
    let nibble = if index % 2 == 0 {
        byte >> 4
    } else {
        byte & 0x0f
    };
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'a' + (nibble - 10),
    }
}

fn md5_hex_bytes(digest: [u8; 16]) -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    let mut index = 0;
    while index < 32 {
        bytes[index] = md5_hex_char_code(digest, index);
        index += 1;
    }
    bytes
}

fn md5_hex_digest(digest: [u8; 16]) -> [u8; 16] {
    md5::Md5::digest(md5_hex_bytes(digest)).into()
}

fn apply_md5_hex_rounds(mut digest: [u8; 16], rounds: usize) -> [u8; 16] {
    let mut remaining = 0;
    while remaining < rounds {
        digest = md5_hex_digest(digest);
        remaining += 1;
    }
    digest
}

fn read_guest_header(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
) -> Result<[u8; 16], HostStatus> {
    let bytes = read_guest_slice(caller, ptr, 16)?;
    let mut header = [0_u8; 16];
    header.copy_from_slice(bytes);
    Ok(header)
}

fn read_guest_slice<'a>(
    caller: &'a mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    len: i32,
) -> Result<&'a [u8], HostStatus> {
    if ptr < 0 || len < 0 {
        return Err(HostStatus::BadGuestMemory);
    }
    let memory = caller
        .data()
        .guest_memory
        .clone()
        .ok_or(HostStatus::BadGuestMemory)?;
    memory
        .data(&*caller)
        .get(ptr as usize..)
        .and_then(|slice| slice.get(..len as usize))
        .ok_or(HostStatus::BadGuestMemory)
}

fn alloc_guest_string(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    text: &str,
) -> Result<i32, HostStatus> {
    let byte_len = i32::try_from(text.len()).map_err(|_| HostStatus::BadGuestMemory)?;
    let char_len = i32::try_from(text.chars().count()).map_err(|_| HostStatus::BadGuestMemory)?;
    let total_size = 8_i32
        .checked_add(byte_len)
        .ok_or(HostStatus::BadGuestMemory)?;

    let alloc = caller
        .data()
        .guest_alloc
        .clone()
        .ok_or(HostStatus::BadGuestMemory)?;
    let ptr = alloc
        .call(&mut *caller, total_size)
        .map_err(|_| HostStatus::BadGuestMemory)?;

    write_guest_i32(caller, ptr, byte_len)?;
    write_guest_i32(caller, ptr + 4, char_len)?;
    write_guest_bytes(caller, ptr + 8, text.as_bytes())?;
    Ok(ptr)
}

fn alloc_guest_md5_string(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    source_ptr: i32,
) -> Result<i32, HostStatus> {
    let alloc = caller
        .data()
        .guest_alloc
        .clone()
        .ok_or(HostStatus::BadGuestMemory)?;
    let ptr = alloc
        .call(&mut *caller, MD5_SPECIAL_STRING_SIZE)
        .map_err(|_| HostStatus::BadGuestMemory)?;

    write_guest_i32(caller, ptr, STRING_SPECIAL_TAG_MASK | 32)?;
    write_guest_i32(caller, ptr + 4, 32)?;
    write_guest_i32(caller, ptr + 8, source_ptr)?;
    write_guest_i32(caller, ptr + 12, STRING_MD5_SENTINEL)?;
    write_guest_bytes(caller, ptr + MD5_DIGEST_OFFSET, &[0_u8; 16])?;
    Ok(ptr)
}

fn alloc_guest_cached_md5_string(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    digest: [u8; 16],
) -> Result<i32, HostStatus> {
    let ptr = alloc_guest_md5_string(caller, MD5_INLINE_DIGEST_SOURCE_PTR)?;
    store_cached_md5_digest(caller, ptr, digest);
    Ok(ptr)
}

fn write_guest_bytes(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    bytes: &[u8],
) -> Result<(), HostStatus> {
    if ptr < 0 {
        return Err(HostStatus::BadGuestMemory);
    }
    let memory = caller
        .data()
        .guest_memory
        .clone()
        .ok_or(HostStatus::BadGuestMemory)?;
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

fn write_guest_i64(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    value: i64,
) -> Result<(), HostStatus> {
    write_guest_bytes(caller, ptr, &value.to_le_bytes())
}

fn write_guest_f64(
    caller: &mut wasmtime::Caller<'_, StoreState>,
    ptr: i32,
    value: f64,
) -> Result<(), HostStatus> {
    write_guest_bytes(caller, ptr, &value.to_le_bytes())
}

fn store_host_error(caller: &mut wasmtime::Caller<'_, StoreState>, message: String) {
    caller.data_mut().last_host_error = Some(message);
}

fn read_program_string(program: &mut WasmProgram, ptr: u32) -> Result<String, WasmRuntimeError> {
    let header = program.read_memory(ptr, 16)?;
    let raw_len = i32::from_le_bytes(header[0..4].try_into().map_err(|_| {
        WasmRuntimeError::GuestMemory("guest string header missing byte length".to_string())
    })?);
    if raw_len >= 0 {
        let bytes = program.read_memory(ptr + 8, raw_len as u32)?;
        return String::from_utf8(bytes)
            .map_err(|err| WasmRuntimeError::GuestMemory(err.to_string()));
    }

    let source_ptr = i32::from_le_bytes(header[8..12].try_into().map_err(|_| {
        WasmRuntimeError::GuestMemory(
            "guest special string header missing source pointer".to_string(),
        )
    })?);
    let rhs_or_sentinel = i32::from_le_bytes(header[12..16].try_into().map_err(|_| {
        WasmRuntimeError::GuestMemory("guest special string header missing rhs pointer".to_string())
    })?);
    if rhs_or_sentinel == STRING_FORWARD_SENTINEL {
        if source_ptr < 0 {
            return Err(WasmRuntimeError::GuestMemory(
                "guest forward string uses invalid source pointer".to_string(),
            ));
        }
        return read_program_string(program, source_ptr as u32);
    }
    if rhs_or_sentinel == STRING_MD5_SENTINEL {
        if source_ptr == MD5_INLINE_DIGEST_SOURCE_PTR {
            let digest_bytes = program.read_memory(ptr + MD5_DIGEST_OFFSET as u32, 16)?;
            let digest: [u8; 16] = digest_bytes.try_into().map_err(|_| {
                WasmRuntimeError::GuestMemory(
                    "guest md5 string missing inline digest bytes".to_string(),
                )
            })?;
            let bytes = md5_hex_bytes(digest);
            return String::from_utf8(bytes.to_vec())
                .map_err(|err| WasmRuntimeError::GuestMemory(err.to_string()));
        }
        if source_ptr < 0 {
            return Err(WasmRuntimeError::GuestMemory(
                "guest md5 string uses invalid source pointer".to_string(),
            ));
        }
        let text = read_program_string(program, source_ptr as u32)?;
        return Ok(format!("{:x}", md5::Md5::digest(text.as_bytes())));
    }
    if rhs_or_sentinel == STRING_SLICE_SENTINEL {
        if source_ptr < 0 {
            return Err(WasmRuntimeError::GuestMemory(
                "guest slice string uses invalid aux pointer".to_string(),
            ));
        }
        let aux = program.read_memory(source_ptr as u32, 8)?;
        let base_ptr = i32::from_le_bytes(aux[0..4].try_into().map_err(|_| {
            WasmRuntimeError::GuestMemory(
                "guest slice string missing source pointer".to_string(),
            )
        })?);
        let start_byte = i32::from_le_bytes(aux[4..8].try_into().map_err(|_| {
            WasmRuntimeError::GuestMemory("guest slice string missing start offset".to_string())
        })?);
        if base_ptr < 0 || start_byte < 0 {
            return Err(WasmRuntimeError::GuestMemory(
                "guest slice string uses invalid source range".to_string(),
            ));
        }
        let len = (raw_len & !STRING_SPECIAL_TAG_MASK) as u32;
        let bytes = program.read_memory(base_ptr as u32 + 8 + start_byte as u32, len)?;
        return String::from_utf8(bytes)
            .map_err(|err| WasmRuntimeError::GuestMemory(err.to_string()));
    }
    if rhs_or_sentinel < 0 {
        return Err(WasmRuntimeError::GuestMemory(
            "guest special string uses invalid sentinel".to_string(),
        ));
    }

    if source_ptr < 0 {
        return Err(WasmRuntimeError::GuestMemory(
            "guest concat string uses invalid source pointer".to_string(),
        ));
    }
    let mut text = read_program_string(program, source_ptr as u32)?;
    text.push_str(&read_program_string(program, rhs_or_sentinel as u32)?);
    Ok(text)
}
