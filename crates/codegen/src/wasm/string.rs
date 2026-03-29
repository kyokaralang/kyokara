//! Internal string helpers emitted as WASM functions.

use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

pub const STRING_SPECIAL_TAG_MASK: i32 = i32::MIN;
pub const STRING_SPECIAL_LEN_MASK: i32 = i32::MAX;
pub const STRING_FORWARD_SENTINEL: i32 = -1;
pub const STRING_MD5_SENTINEL: i32 = -2;

/// Emit `$string_flatten_into(src: i32, dst: i32) -> i32`.
///
/// Returns the next free destination pointer after copying `src` into `dst`.
pub fn emit_flatten_into_function(
    flatten_into_fn_index: u32,
    string_md5_materialize_fn_index: Option<u32>,
) -> Function {
    let materialize_fn_index = string_md5_materialize_fn_index.unwrap_or(flatten_into_fn_index);
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0 = src ptr
    // local 1 = dst ptr
    // local 2 = raw len
    // local 3 = aux / rhs ptr / sentinel
    // local 4 = mid / flat ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 12,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(STRING_FORWARD_SENTINEL));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(STRING_MD5_SENTINEL));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(materialize_fn_index));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(STRING_SPECIAL_LEN_MASK));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(flatten_into_fn_index));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(flatten_into_fn_index));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func
}

/// Emit `$string_flatten(ptr: i32) -> i32`.
pub fn emit_flatten_function(
    alloc_fn_index: u32,
    flatten_into_fn_index: u32,
    string_md5_materialize_fn_index: Option<u32>,
) -> Function {
    let materialize_fn_index = string_md5_materialize_fn_index.unwrap_or(flatten_into_fn_index);
    let mut func = Function::new([(4, ValType::I32)]);
    // local 0 = src ptr
    // local 1 = raw len / actual len
    // local 2 = char len
    // local 3 = sentinel / rhs ptr
    // local 4 = out ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 12,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(STRING_FORWARD_SENTINEL));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(STRING_SPECIAL_LEN_MASK));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(STRING_MD5_SENTINEL));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(materialize_fn_index));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(flatten_into_fn_index));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(STRING_FORWARD_SENTINEL));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 12,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func
}
