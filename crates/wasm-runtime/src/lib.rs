//! WASM execution support for Kyokara.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WasmRuntimeError {
    #[error("invalid WASM module: {0}")]
    InvalidModule(#[source] wasmtime::Error),
    #[error("WASM instantiation failed: {0}")]
    Instantiation(#[source] wasmtime::Error),
    #[error("missing or invalid `main` export: {0}")]
    MissingMain(#[source] wasmtime::Error),
    #[error("WASM `main` trapped: {0}")]
    Trap(#[source] wasmtime::Error),
}

/// Instantiated WASM program ready to invoke `main`.
pub struct WasmProgram {
    store: wasmtime::Store<()>,
    instance: wasmtime::Instance,
}

impl WasmProgram {
    pub fn instantiate(bytes: &[u8]) -> Result<Self, WasmRuntimeError> {
        let engine = wasmtime::Engine::default();
        let wasm_module =
            wasmtime::Module::new(&engine, bytes).map_err(WasmRuntimeError::InvalidModule)?;
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &wasm_module, &[])
            .map_err(WasmRuntimeError::Instantiation)?;
        Ok(Self { store, instance })
    }

    pub fn call_main_i64(&mut self) -> Result<i64, WasmRuntimeError> {
        let main_fn = self
            .instance
            .get_typed_func::<(), i64>(&mut self.store, "main")
            .map_err(WasmRuntimeError::MissingMain)?;
        main_fn
            .call(&mut self.store, ())
            .map_err(WasmRuntimeError::Trap)
    }

    pub fn call_main_f64(&mut self) -> Result<f64, WasmRuntimeError> {
        let main_fn = self
            .instance
            .get_typed_func::<(), f64>(&mut self.store, "main")
            .map_err(WasmRuntimeError::MissingMain)?;
        main_fn
            .call(&mut self.store, ())
            .map_err(WasmRuntimeError::Trap)
    }

    pub fn call_main_i32(&mut self) -> Result<i32, WasmRuntimeError> {
        let main_fn = self
            .instance
            .get_typed_func::<(), i32>(&mut self.store, "main")
            .map_err(WasmRuntimeError::MissingMain)?;
        main_fn
            .call(&mut self.store, ())
            .map_err(WasmRuntimeError::Trap)
    }
}
