//! Internal sequence helpers emitted as WASM functions.

use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

fn emit_alloc_exact_list(
    func: &mut Function,
    alloc_fn_index: u32,
    len_local: u32,
    list_local: u32,
    data_local: u32,
    temp_local: u32,
) {
    func.instruction(&Instruction::LocalGet(len_local));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(len_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::LocalSet(temp_local));
    func.instruction(&Instruction::LocalGet(temp_local));
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::LocalGet(temp_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(data_local));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(data_local));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(12));
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::I32Const(12));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(list_local));

    func.instruction(&Instruction::LocalGet(list_local));
    func.instruction(&Instruction::LocalGet(len_local));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(list_local));
    func.instruction(&Instruction::LocalGet(len_local));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(list_local));
    func.instruction(&Instruction::LocalGet(data_local));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
}

fn emit_store_string_slot(
    func: &mut Function,
    data_local: u32,
    slot_local: u32,
    string_local: u32,
) {
    func.instruction(&Instruction::LocalGet(data_local));
    func.instruction(&Instruction::LocalGet(slot_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(string_local));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(slot_local));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(slot_local));
}

fn emit_alloc_empty_string(func: &mut Function, alloc_fn_index: u32, out_local: u32) {
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Sub);
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

fn emit_string_range_alloc(
    func: &mut Function,
    alloc_fn_index: u32,
    base_string_local: u32,
    slice_start_local: u32,
    slice_end_local: u32,
    result_local: u32,
    char_count_local: u32,
    scan_local: u32,
    temp_local: u32,
    byte_local: u32,
) {
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(char_count_local));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(scan_local));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(scan_local));
    func.instruction(&Instruction::LocalGet(slice_end_local));
    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::LocalGet(scan_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(byte_local));
    emit_utf8_width_from_absolute_index(func, base_string_local, byte_local, temp_local, result_local);

    func.instruction(&Instruction::LocalGet(scan_local));
    func.instruction(&Instruction::LocalGet(temp_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(scan_local));
    func.instruction(&Instruction::LocalGet(char_count_local));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(char_count_local));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(slice_end_local));
    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::LocalGet(slice_end_local));
    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(result_local));

    func.instruction(&Instruction::LocalGet(result_local));
    func.instruction(&Instruction::LocalGet(slice_end_local));
    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(result_local));
    func.instruction(&Instruction::LocalGet(char_count_local));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(result_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(base_string_local));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(slice_end_local));
    func.instruction(&Instruction::LocalGet(slice_start_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });
}

pub fn emit_lines_to_list_function(alloc_fn_index: u32, flatten_fn_index: u32) -> Function {
    let mut func = Function::new([(13, ValType::I32)]);
    // param 0 = string ptr
    // local 1 = byte len
    // local 2 = scan idx
    // local 3 = segment start
    // local 4 = segment count
    // local 5 = list ptr
    // local 6 = data ptr
    // local 7 = slot idx
    // local 8 = temp width / size
    // local 9 = segment end
    // local 10 = char count
    // local 11 = char scan
    // local 12 = result string
    // local 13 = temp byte / absolute index

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

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(10));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::End);

    emit_alloc_exact_list(&mut func, alloc_fn_index, 4, 5, 6, 8);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(10));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalSet(9));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(13));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    emit_string_range_alloc(&mut func, alloc_fn_index, 0, 3, 9, 12, 10, 11, 8, 13);
    emit_store_string_slot(&mut func, 6, 7, 12);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    emit_string_range_alloc(&mut func, alloc_fn_index, 0, 3, 1, 12, 10, 11, 8, 13);
    emit_store_string_slot(&mut func, 6, 7, 12);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}

pub fn emit_split_to_list_function(alloc_fn_index: u32, flatten_fn_index: u32) -> Function {
    let mut func = Function::new([(18, ValType::I32)]);
    // param 0 = split seq ptr
    // local 1 = string ptr
    // local 2 = delim ptr
    // local 3 = string byte len
    // local 4 = delim byte len
    // local 5 = segment count
    // local 6 = scan idx
    // local 7 = segment start
    // local 8 = list ptr
    // local 9 = data ptr
    // local 10 = slot idx
    // local 11 = inner offset
    // local 12 = matched / temp end
    // local 13 = result string
    // local 14 = char count
    // local 15 = char scan
    // local 16 = temp width / size
    // local 17 = temp byte
    // local 18 = shared empty string

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Call(flatten_fn_index));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 12,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Call(flatten_fn_index));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    emit_alloc_exact_list(&mut func, alloc_fn_index, 5, 8, 9, 16);
    emit_alloc_empty_string(&mut func, alloc_fn_index, 18);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(10));
    emit_store_string_slot(&mut func, 9, 10, 18);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    emit_utf8_width_from_absolute_index(&mut func, 1, 6, 16, 17);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(12));
    emit_string_range_alloc(&mut func, alloc_fn_index, 1, 6, 12, 13, 14, 15, 16, 17);
    emit_store_string_slot(&mut func, 9, 10, 13);
    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    emit_store_string_slot(&mut func, 9, 10, 18);
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(11));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    emit_alloc_exact_list(&mut func, alloc_fn_index, 5, 8, 9, 16);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(10));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(11));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::If(BlockType::Empty));
    emit_string_range_alloc(&mut func, alloc_fn_index, 1, 7, 6, 13, 14, 15, 16, 17);
    emit_store_string_slot(&mut func, 9, 10, 13);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    emit_string_range_alloc(&mut func, alloc_fn_index, 1, 7, 3, 13, 14, 15, 16, 17);
    emit_store_string_slot(&mut func, 9, 10, 13);
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}
