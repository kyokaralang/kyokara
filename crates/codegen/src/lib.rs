//! WASM code generation from KyokaraIR.
//!
//! Compiles a KIR module to a WASM binary.
//!
//! Current `main` supports the frozen surface across scalars, control flow,
//! function calls, closures, ADTs, records, strings, collections, trait-backed
//! builtins, and the shared replay/capability host ABI. Public single-file and
//! project/package-mode Wasm run/build/replay now flow through this backend.

pub mod error;
pub mod wasm;

use kyokara_hir_def::item_tree::ItemTree;
use kyokara_intern::Interner;
use kyokara_kir::KirModule;

use crate::error::CodegenError;

/// Compile a KIR module to WASM bytecode.
///
/// Returns the raw WASM binary (`Vec<u8>`) suitable for loading into
/// a WASM runtime (e.g. wasmtime).
pub fn compile(
    kir: &KirModule,
    item_tree: &ItemTree,
    interner: &Interner,
) -> Result<Vec<u8>, CodegenError> {
    wasm::compile_module(kir, item_tree, interner)
}
