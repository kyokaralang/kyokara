//! Internal list helpers emitted as WASM functions.

use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

fn emit_alloc_dynamic_bytes_to_local(
    func: &mut Function,
    alloc_fn_index: u32,
    size_local: u32,
    out_local: u32,
) {
    func.instruction(&Instruction::LocalGet(size_local));
    func.instruction(&Instruction::Call(alloc_fn_index));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::LocalGet(size_local));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(out_local));
}

pub fn emit_mutable_list_push_i64_function(alloc_fn_index: u32) -> Function {
    let mut func = Function::new([(9, ValType::I32)]);
    // param 0 = list ptr
    // param 1 = value bits
    // local 2 = len
    // local 3 = capacity
    // local 4 = data ptr
    // local 5 = new len
    // local 6 = new capacity / byte size
    // local 7 = temp alloc ptr
    // local 8 = old data end
    // local 9 = heap ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::LocalSet(7));
    emit_alloc_dynamic_bytes_to_local(&mut func, alloc_fn_index, 7, 7);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::LocalSet(7));
    emit_alloc_dynamic_bytes_to_local(&mut func, alloc_fn_index, 7, 7);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::MemoryCopy {
        src_mem: 0,
        dst_mem: 0,
    });
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}

pub fn emit_mutable_list_set_i64_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // param 0 = list ptr
    // param 1 = raw index
    // param 2 = value bits
    // local 3 = len
    // local 4 = index i32
    // local 5 = data ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64GeU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}

pub fn emit_list_index_i64_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // param 0 = list ptr
    // param 1 = raw index
    // local 2 = len
    // local 3 = index i32
    // local 4 = data ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64GeU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}

pub fn emit_list_get_i64_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // param 0 = list ptr
    // param 1 = raw index
    // local 2 = len
    // local 3 = index i32
    // local 4 = data ptr

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64GeU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}

pub fn emit_mutable_list_pop_i64_function() -> Function {
    let mut func = Function::new([(4, ValType::I32), (1, ValType::I64)]);
    // param 0 = list ptr
    // local 1 = len
    // local 2 = capacity
    // local 3 = data ptr
    // local 4 = new len
    // local 5 = removed value bits

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 8,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func
}
