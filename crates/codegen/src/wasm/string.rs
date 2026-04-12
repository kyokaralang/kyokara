//! Internal string helpers emitted as WASM functions.

use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

pub const STRING_SPECIAL_TAG_MASK: i32 = i32::MIN;
pub const STRING_SPECIAL_LEN_MASK: i32 = i32::MAX;
pub const STRING_FORWARD_SENTINEL: i32 = -1;
pub const STRING_MD5_SENTINEL: i32 = -2;
pub const STRING_SLICE_SENTINEL: i32 = -3;

fn emit_alloc_empty_string(func: &mut Function, alloc_fn_index: u32, out_local: u32) {
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::LocalSet(out_local));
    func.instruction(&Instruction::LocalGet(out_local));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
}

fn emit_utf8_width_from_absolute_index(
    func: &mut Function,
    string_local: u32,
    byte_index_local: u32,
    width_local: u32,
    byte_local: u32,
) {
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(byte_local));

    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::LocalSet(width_local));
    func.instruction(&Instruction::LocalGet(byte_local));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::LocalSet(width_local));
    func.instruction(&Instruction::LocalGet(byte_local));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::LocalSet(width_local));
    func.instruction(&Instruction::LocalGet(byte_local));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(width_local));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
}

fn emit_utf8_codepoint_from_absolute_index(
    func: &mut Function,
    string_local: u32,
    byte_index_local: u32,
    width_local: u32,
    dst_local: u32,
) {
    func.instruction(&Instruction::LocalGet(width_local));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(dst_local));
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(width_local));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x1F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(6));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x3F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalSet(dst_local));
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(width_local));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x0F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(12));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x3F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(6));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x3F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalSet(dst_local));
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x07));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(18));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x3F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(12));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x3F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(6));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(byte_index_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0x3F));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalSet(dst_local));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
}

fn emit_trim_whitespace_check_from_absolute_index(
    func: &mut Function,
    string_local: u32,
    byte_index_local: u32,
    width_local: u32,
    dst_local: u32,
) {
    emit_utf8_codepoint_from_absolute_index(
        func,
        string_local,
        byte_index_local,
        width_local,
        dst_local,
    );

    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x09));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x0D));
    func.instruction(&Instruction::I32LeU);
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x20));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x85));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0xA0));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x1680));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x2000));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x200A));
    func.instruction(&Instruction::I32LeU);
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x2028));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x2029));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x202F));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x205F));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalGet(dst_local));
    func.instruction(&Instruction::I32Const(0x3000));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::I32Or);
    func.instruction(&Instruction::LocalSet(dst_local));
}

/// Emit `$string_flatten_into(src: i32, dst: i32) -> i32`.
///
/// Returns the next free destination pointer after copying `src` into `dst`.
pub fn emit_flatten_into_function(
    flatten_into_fn_index: u32,
    string_md5_materialize_fn_index: Option<u32>,
) -> Function {
    let materialize_fn_index = string_md5_materialize_fn_index.unwrap_or(flatten_into_fn_index);
    let mut func = Function::new([(4, ValType::I32)]);
    // local 0 = src ptr
    // local 1 = dst ptr
    // local 2 = raw len
    // local 3 = aux / rhs ptr / sentinel
    // local 4 = mid / flat ptr / aux ptr
    // local 5 = slice source ptr / start byte

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

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(STRING_SLICE_SENTINEL));
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
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(STRING_SPECIAL_LEN_MASK));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });
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

/// Emit `$string_trim(ptr: i32) -> i32`.
pub fn emit_trim_function(alloc_fn_index: u32, flatten_fn_index: u32) -> Function {
    let mut func = Function::new([(10, ValType::I32)]);
    // param 0 = string ptr
    // local 1 = byte len
    // local 2 = char len
    // local 3 = start byte
    // local 4 = start char
    // local 5 = temp width / trimmed byte len
    // local 6 = codepoint / whitespace flag
    // local 7 = end byte
    // local 8 = end char
    // local 9 = prev byte
    // local 10 = result ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(flatten_fn_index));
    func.instruction(&Instruction::LocalSet(0));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    emit_utf8_width_from_absolute_index(&mut func, 0, 3, 5, 6);
    emit_trim_whitespace_check_from_absolute_index(&mut func, 0, 3, 5, 6);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    emit_alloc_empty_string(&mut func, alloc_fn_index, 10);
    func.instruction(&Instruction::Else);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalSet(8));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32LeU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(9));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0xC0));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    emit_utf8_width_from_absolute_index(&mut func, 0, 9, 2, 6);
    emit_trim_whitespace_check_from_absolute_index(&mut func, 0, 9, 2, 6);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::LocalSet(10));

    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}
