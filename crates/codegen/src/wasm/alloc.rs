//! Bump allocator emitted as a WASM function.

use wasm_encoder::{BlockType, Function, Instruction, ValType};

/// Emit the `$alloc(size: i32) -> i32` bump allocator function body.
///
/// Algorithm:
///   ptr = global $heap_ptr
///   reject if size < 0
///   new_ptr = ptr + size
///   reject on wrapping add (new_ptr < ptr)
///   grow memory if new_ptr exceeds current memory size
///   global $heap_ptr = new_ptr
///   return ptr
pub fn emit_alloc_function() -> Function {
    let mut func = Function::new([(2, ValType::I32)]); // locals: ptr, new_ptr
    // local 0 = param `size`
    // local 1 = `ptr`
    // local 2 = `new_ptr`

    // ptr = global.get $heap_ptr
    func.instruction(&Instruction::GlobalGet(0));
    func.instruction(&Instruction::LocalSet(1));

    // Reject negative sizes.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // new_ptr = ptr + size
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));

    // Reject wrapping additions.
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // Grow memory when the requested allocation would exceed the current size.
    // Compare in i64 space to avoid overflow when converting pages -> bytes.
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::MemorySize(0));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Const(65_536));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::I64GtU);
    func.instruction(&Instruction::If(BlockType::Empty));
    // pages_to_grow = ceil((new_ptr - current_bytes) / 65536)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::MemorySize(0));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Const(65_536));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::I64Sub);
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Sub);
    func.instruction(&Instruction::I64Const(65_536));
    func.instruction(&Instruction::I64DivU);
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::MemoryGrow(0));
    func.instruction(&Instruction::I32Const(-1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // global $heap_ptr = new_ptr
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::GlobalSet(0));

    // return ptr
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);

    func
}

#[cfg(test)]
mod tests {
    use super::emit_alloc_function;
    use wasm_encoder::{
        CodeSection, ConstExpr, ExportKind, ExportSection, FunctionSection, GlobalSection,
        GlobalType, MemorySection, MemoryType, Module, TypeSection, ValType,
    };

    fn instantiate_alloc_module() -> (wasmtime::Store<()>, wasmtime::Instance) {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function([ValType::I32], [ValType::I32]);

        let mut funcs = FunctionSection::new();
        funcs.function(0);

        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });

        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );

        let mut exports = ExportSection::new();
        exports.export("alloc", ExportKind::Func, 0);
        exports.export("heap_ptr", ExportKind::Global, 0);
        exports.export("memory", ExportKind::Memory, 0);

        let mut code = CodeSection::new();
        code.function(&emit_alloc_function());

        module.section(&types);
        module.section(&funcs);
        module.section(&memories);
        module.section(&globals);
        module.section(&exports);
        module.section(&code);

        let engine = wasmtime::Engine::default();
        let wasm_module =
            wasmtime::Module::new(&engine, module.finish()).expect("valid alloc test module");
        let mut store = wasmtime::Store::new(&engine, ());
        let instance =
            wasmtime::Instance::new(&mut store, &wasm_module, &[]).expect("instance should load");
        (store, instance)
    }

    fn call_alloc(
        store: &mut wasmtime::Store<()>,
        instance: &wasmtime::Instance,
        size: i32,
    ) -> Result<i32, wasmtime::Error> {
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut *store, "alloc")
            .expect("alloc export should exist");
        alloc.call(store, size)
    }

    fn heap_ptr_get(store: &mut wasmtime::Store<()>, instance: &wasmtime::Instance) -> i32 {
        let global = instance
            .get_global(&mut *store, "heap_ptr")
            .expect("heap_ptr export should exist");
        global
            .get(store)
            .i32()
            .expect("heap_ptr should be exported as i32 global")
    }

    fn heap_ptr_set(store: &mut wasmtime::Store<()>, instance: &wasmtime::Instance, value: i32) {
        let global = instance
            .get_global(&mut *store, "heap_ptr")
            .expect("heap_ptr export should exist");
        global
            .set(store, wasmtime::Val::I32(value))
            .expect("heap_ptr should be mutable");
    }

    fn memory_pages_get(store: &mut wasmtime::Store<()>, instance: &wasmtime::Instance) -> u64 {
        instance
            .get_memory(&mut *store, "memory")
            .expect("memory export should exist")
            .size(store)
    }

    #[test]
    fn alloc_positive_size_returns_old_ptr_and_advances_heap() {
        let (mut store, instance) = instantiate_alloc_module();
        assert_eq!(heap_ptr_get(&mut store, &instance), 0);

        assert_eq!(
            call_alloc(&mut store, &instance, 16).expect("positive alloc(16) should succeed"),
            0
        );
        assert_eq!(heap_ptr_get(&mut store, &instance), 16);

        assert_eq!(
            call_alloc(&mut store, &instance, 8).expect("positive alloc(8) should succeed"),
            16
        );
        assert_eq!(heap_ptr_get(&mut store, &instance), 24);
    }

    #[test]
    fn alloc_negative_size_traps() {
        let (mut store, instance) = instantiate_alloc_module();

        assert!(call_alloc(&mut store, &instance, -1).is_err());
        assert_eq!(
            heap_ptr_get(&mut store, &instance),
            0,
            "heap_ptr should remain unchanged on rejected allocation"
        );
    }

    #[test]
    fn alloc_wrapping_addition_traps() {
        let (mut store, instance) = instantiate_alloc_module();
        let start = -8;
        heap_ptr_set(&mut store, &instance, start);

        assert!(call_alloc(&mut store, &instance, 16).is_err());
        assert_eq!(
            heap_ptr_get(&mut store, &instance),
            start,
            "heap_ptr should remain unchanged on overflow"
        );
    }

    #[test]
    fn alloc_beyond_memory_limit_grows_memory() {
        let (mut store, instance) = instantiate_alloc_module();
        // Memory is one page = 65536 bytes.
        heap_ptr_set(&mut store, &instance, 65_520);
        assert_eq!(memory_pages_get(&mut store, &instance), 1);

        assert_eq!(
            call_alloc(&mut store, &instance, 32).expect("alloc should grow guest memory"),
            65_520
        );
        assert_eq!(
            heap_ptr_get(&mut store, &instance),
            65_552,
            "heap_ptr should advance after growth"
        );
        assert_eq!(memory_pages_get(&mut store, &instance), 2);
    }
}
