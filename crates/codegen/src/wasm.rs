//! WASM module assembly.

pub mod alloc;
pub mod control;
pub mod func;
pub mod layout;
pub mod ty;

use kyokara_hir_def::item_tree::{ItemTree, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;
use kyokara_kir::KirModule;
use rustc_hash::FxHashMap;
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, FunctionSection, GlobalSection, GlobalType,
    MemorySection, MemoryType, Module, TypeSection, ValType,
};

use crate::error::CodegenError;
use crate::wasm::func::FuncCodegen;
use crate::wasm::layout::AdtLayout;
use crate::wasm::ty::ty_to_valtype;

/// Shared state during module codegen.
pub struct ModuleCtx<'a> {
    pub item_tree: &'a ItemTree,
    pub interner: &'a Interner,
    /// KIR FnId (name) → WASM function index.
    pub fn_name_map: FxHashMap<Name, u32>,
    /// ADT type index → precomputed layout.
    pub adt_layouts: FxHashMap<TypeItemIdx, AdtLayout>,
    /// WASM function index of the $alloc builtin.
    pub alloc_fn_index: u32,
}

/// Compile a KIR module to a WASM binary.
pub fn compile_module(
    kir: &KirModule,
    item_tree: &ItemTree,
    interner: &Interner,
) -> Result<Vec<u8>, CodegenError> {
    let adt_layouts = layout::compute_adt_layouts(item_tree, interner);

    // Function index 0 = $alloc
    let alloc_fn_index = 0u32;
    let mut next_fn_index = 1u32;

    // Build function name → index map.
    let mut fn_name_map = FxHashMap::default();
    let mut fn_order = Vec::new(); // (kir FnId, wasm index)

    for (fn_id, kir_func) in kir.functions.iter() {
        fn_name_map.insert(kir_func.name, next_fn_index);
        fn_order.push((fn_id, next_fn_index));
        next_fn_index += 1;
    }

    let ctx = ModuleCtx {
        item_tree,
        interner,
        fn_name_map,
        adt_layouts,
        alloc_fn_index,
    };

    // ── Type section ──────────────────────────────────────────────

    let mut types = TypeSection::new();

    // Type 0: alloc(i32) -> i32
    types.ty().function([ValType::I32], [ValType::I32]);

    // Build type index for each KIR function.
    let mut fn_type_indices = Vec::new();
    for (fn_id, _wasm_idx) in &fn_order {
        let kir_func = &kir.functions[*fn_id];
        let params: Vec<ValType> = kir_func
            .params
            .iter()
            .map(|(_, ty)| ty_to_valtype(ty))
            .collect::<Result<_, _>>()?;
        let results: Vec<ValType> = if matches!(kir_func.ret_ty, kyokara_hir_ty::ty::Ty::Unit) {
            // Unit-returning functions have no WASM return value... actually,
            // we DO return i32(0) for Unit, so include it.
            vec![ty_to_valtype(&kir_func.ret_ty)?]
        } else {
            vec![ty_to_valtype(&kir_func.ret_ty)?]
        };

        let type_idx = types.len();
        types.ty().function(params, results);
        fn_type_indices.push(type_idx);
    }

    // ── Function section ──────────────────────────────────────────

    let mut functions = FunctionSection::new();

    // Function 0: alloc (type 0)
    functions.function(0);

    // User functions
    for &type_idx in &fn_type_indices {
        functions.function(type_idx);
    }

    // ── Memory section ────────────────────────────────────────────

    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });

    // ── Global section ────────────────────────────────────────────

    let mut globals = GlobalSection::new();
    // Global 0: $heap_ptr (mutable i32, initial 0)
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &wasm_encoder::ConstExpr::i32_const(0),
    );

    // ── Export section ────────────────────────────────────────────

    let mut exports = ExportSection::new();

    // Export memory
    exports.export("memory", ExportKind::Memory, 0);

    // Export all user functions by name.
    for (fn_id, wasm_idx) in &fn_order {
        let kir_func = &kir.functions[*fn_id];
        let name = kir_func.name.resolve(interner);
        exports.export(name, ExportKind::Func, *wasm_idx);
    }

    // ── Code section ──────────────────────────────────────────────

    let mut code = CodeSection::new();

    // Function 0: $alloc
    code.function(&alloc::emit_alloc_function());

    // User functions
    for (fn_id, _wasm_idx) in &fn_order {
        let kir_func = &kir.functions[*fn_id];
        let codegen = FuncCodegen::new(kir_func, &ctx);
        let wasm_func = codegen.emit()?;
        code.function(&wasm_func);
    }

    // ── Assemble module ───────────────────────────────────────────

    let mut module = Module::new();
    module.section(&types);
    module.section(&functions);
    module.section(&memories);
    module.section(&globals);
    module.section(&exports);
    module.section(&code);

    Ok(module.finish())
}
