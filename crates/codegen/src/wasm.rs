//! WASM module assembly.

pub mod alloc;
pub mod control;
pub mod func;
pub mod layout;
pub mod ty;

use kyokara_hir_def::item_tree::{ItemTree, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::ty::Ty;
use kyokara_intern::Interner;
use kyokara_kir::KirModule;
use rustc_hash::FxHashMap;
use wasm_encoder::{
    CodeSection, ElementSection, Elements, EntityType, ExportKind, ExportSection, FunctionSection,
    GlobalSection, GlobalType, ImportSection, MemorySection, MemoryType, Module, RefType,
    TableSection, TableType, TypeSection, ValType,
};

use crate::error::CodegenError;
use crate::wasm::func::FuncCodegen;
use crate::wasm::layout::AdtLayout;
use crate::wasm::ty::ty_to_valtype;
use std::borrow::Cow;

const HOST_MODULE: &str = "kyokara_host";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnTypeKey {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

impl FnTypeKey {
    pub fn from_ty(ty: &Ty) -> Result<Self, CodegenError> {
        let Ty::Fn { params, ret } = ty else {
            return Err(CodegenError::UnsupportedType(format!("{ty:?}")));
        };
        Ok(Self {
            params: params
                .iter()
                .map(ty_to_valtype)
                .collect::<Result<Vec<_>, _>>()?,
            results: vec![ty_to_valtype(ret)?],
        })
    }
}

/// Shared state during module codegen.
pub struct ModuleCtx<'a> {
    pub item_tree: &'a ItemTree,
    pub interner: &'a Interner,
    /// KIR FnId (name) → WASM function index.
    pub fn_name_map: FxHashMap<Name, u32>,
    /// KIR function name -> table slot for first-class function references.
    pub fn_table_map: FxHashMap<Name, u32>,
    /// Structural function signature -> type section index.
    pub fn_type_map: FxHashMap<FnTypeKey, u32>,
    /// ADT type index → precomputed layout.
    pub adt_layouts: FxHashMap<TypeItemIdx, AdtLayout>,
    /// WASM function index of the $alloc builtin.
    pub alloc_fn_index: u32,
    pub string_to_upper_fn_index: Option<u32>,
    pub string_to_lower_fn_index: Option<u32>,
    pub string_md5_fn_index: Option<u32>,
    pub parse_int_fn_index: Option<u32>,
    pub parse_float_fn_index: Option<u32>,
}

/// Compile a KIR module to a WASM binary.
pub fn compile_module(
    kir: &KirModule,
    item_tree: &ItemTree,
    interner: &Interner,
) -> Result<Vec<u8>, CodegenError> {
    let adt_layouts = layout::compute_adt_layouts(item_tree, interner);

    let needs_string_to_upper = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "string_to_upper"
            )
        })
    });
    let needs_string_to_lower = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "string_to_lower"
            )
        })
    });
    let needs_string_md5 = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "string_md5"
            )
        })
    });
    let needs_parse_int = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "parse_int"
            )
        })
    });
    let needs_parse_float = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "parse_float"
            )
        })
    });

    let mut next_fn_index = 0u32;
    let string_to_upper_fn_index = if needs_string_to_upper {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let string_to_lower_fn_index = if needs_string_to_lower {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let string_md5_fn_index = if needs_string_md5 {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let parse_int_fn_index = if needs_parse_int {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let parse_float_fn_index = if needs_parse_float {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let alloc_fn_index = next_fn_index;
    next_fn_index += 1;

    // Build function name → index map.
    let mut fn_name_map = FxHashMap::default();
    let mut fn_table_map = FxHashMap::default();
    let mut fn_order = Vec::new(); // (kir FnId, wasm index)

    for (table_idx, (fn_id, kir_func)) in kir.functions.iter().enumerate() {
        fn_name_map.insert(kir_func.name, next_fn_index);
        fn_table_map.insert(kir_func.name, table_idx as u32);
        fn_order.push((fn_id, next_fn_index));
        next_fn_index += 1;
    }

    let mut fn_type_map = FxHashMap::default();

    // ── Type section ──────────────────────────────────────────────

    let mut types = TypeSection::new();

    // Type 0: alloc(i32) -> i32
    types.ty().function([ValType::I32], [ValType::I32]);
    // Type 1: string helper(i32 ptr, i32 len) -> i32 ptr
    types
        .ty()
        .function([ValType::I32, ValType::I32], [ValType::I32]);
    // Type 2: parse helper(i32 ptr, i32 len, i32 value_out, i32 msg_out) -> i32 status
    types.ty().function(
        [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
        [ValType::I32],
    );

    // Build type index for each KIR function, deduplicated by structural
    // signature so indirect calls can reuse the same type index.
    let mut fn_type_indices = Vec::new();
    for (fn_id, _wasm_idx) in &fn_order {
        let kir_func = &kir.functions[*fn_id];
        let params: Vec<ValType> = kir_func
            .params
            .iter()
            .map(|(_, ty)| ty_to_valtype(ty))
            .collect::<Result<_, _>>()?;
        let results = vec![ty_to_valtype(&kir_func.ret_ty)?];
        let key = FnTypeKey {
            params: params.clone(),
            results: results.clone(),
        };
        let type_idx = if let Some(&existing) = fn_type_map.get(&key) {
            existing
        } else {
            let type_idx = types.len();
            types.ty().function(params, results);
            fn_type_map.insert(key, type_idx);
            type_idx
        };
        fn_type_indices.push(type_idx);
    }

    let ctx = ModuleCtx {
        item_tree,
        interner,
        fn_name_map,
        fn_table_map,
        fn_type_map,
        adt_layouts,
        alloc_fn_index,
        string_to_upper_fn_index,
        string_to_lower_fn_index,
        string_md5_fn_index,
        parse_int_fn_index,
        parse_float_fn_index,
    };

    // ── Import section ────────────────────────────────────────────

    let mut imports = ImportSection::new();
    if string_to_upper_fn_index.is_some() {
        imports.import(HOST_MODULE, "string_to_upper", EntityType::Function(1));
    }
    if string_to_lower_fn_index.is_some() {
        imports.import(HOST_MODULE, "string_to_lower", EntityType::Function(1));
    }
    if string_md5_fn_index.is_some() {
        imports.import(HOST_MODULE, "string_md5", EntityType::Function(1));
    }
    if parse_int_fn_index.is_some() {
        imports.import(HOST_MODULE, "parse_int", EntityType::Function(2));
    }
    if parse_float_fn_index.is_some() {
        imports.import(HOST_MODULE, "parse_float", EntityType::Function(2));
    }

    // ── Function section ──────────────────────────────────────────

    let mut functions = FunctionSection::new();

    // Function 0: alloc (type 0)
    functions.function(0);

    // User functions
    for &type_idx in &fn_type_indices {
        functions.function(type_idx);
    }

    // ── Table / element sections ─────────────────────────────────

    let mut tables = TableSection::new();
    let mut elements = ElementSection::new();
    if !fn_order.is_empty() {
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: fn_order.len() as u64,
            maximum: Some(fn_order.len() as u64),
            shared: false,
        });
        let function_indices: Vec<u32> = fn_order.iter().map(|(_, wasm_idx)| *wasm_idx).collect();
        elements.active(
            None,
            &wasm_encoder::ConstExpr::i32_const(0),
            Elements::Functions(Cow::Owned(function_indices)),
        );
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
    exports.export("__kyokara_alloc", ExportKind::Func, alloc_fn_index);

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
    if !imports.is_empty() {
        module.section(&imports);
    }
    module.section(&functions);
    if !tables.is_empty() {
        module.section(&tables);
    }
    module.section(&memories);
    module.section(&globals);
    module.section(&exports);
    if !elements.is_empty() {
        module.section(&elements);
    }
    module.section(&code);

    Ok(module.finish())
}
