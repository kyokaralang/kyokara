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
use kyokara_kir::function::KirFunction;
use rustc_hash::FxHashMap;
use wasm_encoder::{
    CodeSection, ElementSection, Elements, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, Instruction, MemArg, MemorySection,
    MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
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
    pub fs_read_file_fn_index: Option<u32>,
    pub io_print_fn_index: Option<u32>,
    pub io_println_fn_index: Option<u32>,
    pub current_closure_global_index: u32,
}

pub const CURRENT_CLOSURE_GLOBAL_INDEX: u32 = 1;

pub fn closure_object_size(capture_count: usize) -> i32 {
    8 * (capture_count as i32 + 1)
}

pub fn closure_capture_offset(index: usize) -> u64 {
    8 + (index as u64 * 8)
}

fn register_fn_type(
    types: &mut TypeSection,
    type_map: &mut FxHashMap<FnTypeKey, u32>,
    params: Vec<ValType>,
    results: Vec<ValType>,
) -> u32 {
    let key = FnTypeKey {
        params: params.clone(),
        results: results.clone(),
    };
    if let Some(&existing) = type_map.get(&key) {
        existing
    } else {
        let type_idx = types.len();
        types.ty().function(params, results);
        type_map.insert(key, type_idx);
        type_idx
    }
}

fn emit_typed_load_static(func: &mut Function, ty: &Ty, offset: u64) {
    match ty {
        Ty::Float => {
            func.instruction(&Instruction::F64Load(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
        }
        Ty::Bool
        | Ty::Unit
        | Ty::Char
        | Ty::String
        | Ty::Fn { .. }
        | Ty::Adt { .. }
        | Ty::Record { .. }
        | Ty::Never
        | Ty::Error => {
            func.instruction(&Instruction::I64Load(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            func.instruction(&Instruction::I32WrapI64);
        }
        _ => {
            func.instruction(&Instruction::I64Load(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
        }
    }
}

fn emit_wrapper_function(
    kir_func: &KirFunction,
    actual_fn_index: u32,
    current_closure_global_index: u32,
) -> Result<Function, CodegenError> {
    let capture_count = kir_func.closure_capture_tys.len();
    let wrapper_param_count = kir_func
        .params
        .len()
        .checked_sub(capture_count)
        .ok_or_else(|| {
            CodegenError::UnsupportedInstruction("closure wrapper param underflow".into())
        })?;

    let mut func = Function::new(Vec::new());
    for index in 0..wrapper_param_count {
        func.instruction(&Instruction::LocalGet(index as u32));
    }
    for (index, capture_ty) in kir_func.closure_capture_tys.iter().enumerate() {
        func.instruction(&Instruction::GlobalGet(current_closure_global_index));
        emit_typed_load_static(&mut func, capture_ty, closure_capture_offset(index));
    }
    func.instruction(&Instruction::Call(actual_fn_index));
    func.instruction(&Instruction::End);
    Ok(func)
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
    let needs_fs_read_file = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "read_file"
            )
        })
    });
    let needs_io_print = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "print"
            )
        })
    });
    let needs_io_println = kir.functions.iter().any(|(_, func)| {
        func.values.iter().any(|(_, value)| {
            matches!(
                &value.inst,
                kyokara_kir::inst::Inst::Call {
                    target: kyokara_kir::inst::CallTarget::Intrinsic(name),
                    ..
                } if name == "println"
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
    let fs_read_file_fn_index = if needs_fs_read_file {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let io_print_fn_index = if needs_io_print {
        let idx = next_fn_index;
        next_fn_index += 1;
        Some(idx)
    } else {
        None
    };
    let io_println_fn_index = if needs_io_println {
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

    for (fn_id, kir_func) in kir.functions.iter() {
        fn_name_map.insert(kir_func.name, next_fn_index);
        fn_order.push((fn_id, next_fn_index));
        next_fn_index += 1;
    }

    let mut wrapper_order = Vec::new(); // (kir FnId, wasm index)
    for (table_idx, (fn_id, kir_func)) in kir.functions.iter().enumerate() {
        fn_table_map.insert(kir_func.name, table_idx as u32);
        wrapper_order.push((fn_id, next_fn_index));
        next_fn_index += 1;
    }

    let mut all_type_map = FxHashMap::default();
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
    // Type 3: fs read helper(path ptr, path len, buffer ptr, buffer cap, written len ptr, required_by ptr, required_by len) -> i32 status
    types.ty().function(
        [
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        [ValType::I32],
    );

    // Build type index for each KIR function, deduplicated by structural
    // signature so indirect calls can reuse the same type index.
    let mut fn_type_indices = Vec::new();
    let mut wrapper_type_indices = Vec::new();
    for (fn_id, _wasm_idx) in &fn_order {
        let kir_func = &kir.functions[*fn_id];
        let params: Vec<ValType> = kir_func
            .params
            .iter()
            .map(|(_, ty)| ty_to_valtype(ty))
            .collect::<Result<_, _>>()?;
        let results = vec![ty_to_valtype(&kir_func.ret_ty)?];
        let type_idx = register_fn_type(&mut types, &mut all_type_map, params, results);
        fn_type_indices.push(type_idx);

        let wrapper_param_count = kir_func
            .params
            .len()
            .saturating_sub(kir_func.closure_capture_tys.len());
        let wrapper_params: Vec<ValType> = kir_func.params[..wrapper_param_count]
            .iter()
            .map(|(_, ty)| ty_to_valtype(ty))
            .collect::<Result<_, _>>()?;
        let wrapper_results = vec![ty_to_valtype(&kir_func.ret_ty)?];
        let wrapper_type_idx = register_fn_type(
            &mut types,
            &mut all_type_map,
            wrapper_params.clone(),
            wrapper_results.clone(),
        );
        wrapper_type_indices.push(wrapper_type_idx);
        fn_type_map.insert(
            FnTypeKey {
                params: wrapper_params,
                results: wrapper_results,
            },
            wrapper_type_idx,
        );
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
        fs_read_file_fn_index,
        io_print_fn_index,
        io_println_fn_index,
        current_closure_global_index: CURRENT_CLOSURE_GLOBAL_INDEX,
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
    if fs_read_file_fn_index.is_some() {
        imports.import(HOST_MODULE, "fs_read_file", EntityType::Function(3));
    }
    if io_print_fn_index.is_some() {
        imports.import(HOST_MODULE, "io_print", EntityType::Function(2));
    }
    if io_println_fn_index.is_some() {
        imports.import(HOST_MODULE, "io_println", EntityType::Function(2));
    }

    // ── Function section ──────────────────────────────────────────

    let mut functions = FunctionSection::new();

    // Function 0: alloc (type 0)
    functions.function(0);

    // User functions
    for &type_idx in &fn_type_indices {
        functions.function(type_idx);
    }
    for &type_idx in &wrapper_type_indices {
        functions.function(type_idx);
    }

    // ── Table / element sections ─────────────────────────────────

    let mut tables = TableSection::new();
    let mut elements = ElementSection::new();
    if !wrapper_order.is_empty() {
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: wrapper_order.len() as u64,
            maximum: Some(wrapper_order.len() as u64),
            shared: false,
        });
        let function_indices: Vec<u32> = wrapper_order
            .iter()
            .map(|(_, wasm_idx)| *wasm_idx)
            .collect();
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
    // Global 1: $current_closure_ptr (mutable i32, initial 0)
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
    for ((fn_id, actual_fn_index), (_wrapper_fn_id, _wrapper_idx)) in
        fn_order.iter().zip(wrapper_order.iter())
    {
        let kir_func = &kir.functions[*fn_id];
        let wrapper_func =
            emit_wrapper_function(kir_func, *actual_fn_index, CURRENT_CLOSURE_GLOBAL_INDEX)?;
        code.function(&wrapper_func);
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
