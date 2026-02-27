//! Bump allocator emitted as a WASM function.

use wasm_encoder::{Function, Instruction, ValType};

/// Emit the `$alloc(size: i32) -> i32` bump allocator function body.
///
/// Algorithm:
///   ptr = global $heap_ptr
///   global $heap_ptr = ptr + size
///   return ptr
pub fn emit_alloc_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]); // 1 local for `ptr`
    // local 0 = param `size`
    // local 1 = `ptr`

    // ptr = global.get $heap_ptr
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::LocalSet(1));

    // global $heap_ptr = ptr + size
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::GlobalSet(0));

    // return ptr
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);

    func
}
