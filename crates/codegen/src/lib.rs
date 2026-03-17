//! WASM code generation from KyokaraIR.
//!
//! Compiles a KIR module to a WASM binary.
//!
//! Current `main` supports the frozen single-file surface across scalars,
//! control flow, function calls, closures, ADTs, records, strings, lists,
//! deques, bitsets, intrinsics, and the shared replay/capability host ABI.
//! The remaining parity work lives in project/package-mode compilation and the
//! still-missing collection + witness-heavy families.

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
