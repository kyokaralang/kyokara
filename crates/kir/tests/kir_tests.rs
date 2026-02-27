//! Tests for KIR data structures, builder, display, and validator.
#![allow(clippy::unwrap_used)]

use kyokara_hir_def::expr::{BinaryOp, UnaryOp};
use kyokara_hir_def::item_tree::ItemTree;
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::effects::EffectSet;
use kyokara_hir_ty::ty::Ty;
use kyokara_intern::Interner;

use kyokara_kir::KirModule;
use kyokara_kir::block::{BranchTarget, Terminator};
use kyokara_kir::build::KirBuilder;
use kyokara_kir::display::{DisplayCtx, display_function, display_module};
use kyokara_kir::function::KirContracts;
use kyokara_kir::inst::{CallTarget, Constant};
use kyokara_kir::validate::validate_function;
use la_arena::Idx;

fn mk_interner() -> Interner {
    Interner::new()
}

fn mk_name(interner: &mut Interner, s: &str) -> Name {
    Name::new(interner, s)
}

// ── Builder + Display: simple identity function ────────────────

#[test]
fn test_identity_function() {
    let mut interner = mk_interner();
    let name = mk_name(&mut interner, "identity");
    let x_name = mk_name(&mut interner, "x");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    // Parameter: x: Int — modeled as a const for the function param
    // In the actual IR, function params are passed as values.
    // For this test, we use a const to represent x.
    let x = b.push_const(Constant::Int(42), Ty::Int);
    b.set_return(x);

    let func = b.build(
        name,
        vec![(x_name, Ty::Int)],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("fn @identity(x: Int) -> Int"));
    assert!(out.contains("const 42 : Int"));
    assert!(out.contains("return %0"));
}

// ── Builder + Display: abs function with branching ─────────────

#[test]
fn test_abs_function_with_branches() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "abs");
    let x_name = mk_name(&mut interner, "x");
    let entry_label = mk_name(&mut interner, "entry");
    let then_label = mk_name(&mut interner, "then");
    let else_label = mk_name(&mut interner, "else");
    let merge_label = mk_name(&mut interner, "merge");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    let then_block = b.new_block(Some(then_label));
    let else_block = b.new_block(Some(else_label));
    let merge_block = b.new_block(Some(merge_label));

    // entry:
    b.switch_to(entry);
    let zero = b.push_const(Constant::Int(0), Ty::Int);
    let x = b.push_const(Constant::Int(1), Ty::Int); // placeholder for param
    let cond = b.push_binary(BinaryOp::Lt, x, zero, Ty::Bool);
    b.set_branch(
        cond,
        BranchTarget {
            block: then_block,
            args: vec![],
        },
        BranchTarget {
            block: else_block,
            args: vec![],
        },
    );

    // then:
    b.switch_to(then_block);
    let neg_x = b.push_unary(UnaryOp::Neg, x, Ty::Int);
    b.set_jump(BranchTarget {
        block: merge_block,
        args: vec![neg_x],
    });

    // else:
    b.switch_to(else_block);
    b.set_jump(BranchTarget {
        block: merge_block,
        args: vec![x],
    });

    // merge(%result: Int):
    let result = b.add_block_param(merge_block, Some(mk_name(&mut interner, "result")), Ty::Int);
    b.switch_to(merge_block);
    b.set_return(result);

    let func = b.build(
        fn_name,
        vec![(x_name, Ty::Int)],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    // Validate
    let diags = validate_function(&func, &interner);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);

    // Display
    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("fn @abs(x: Int) -> Int"));
    assert!(out.contains("branch %2 -> then(), else()"));
    assert!(out.contains("neg %1 : Int"));
    assert!(out.contains("jump -> merge(%3)"));
    assert!(out.contains("jump -> merge(%1)"));
    assert!(out.contains("merge(result: Int):"));
    assert!(out.contains("return result"));
}

// ── Builder + Display: function call ───────────────────────────

#[test]
fn test_call_instruction() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "caller");
    let callee_name = mk_name(&mut interner, "callee");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    let arg = b.push_const(Constant::Int(10), Ty::Int);
    let result = b.push_call(CallTarget::Direct(callee_name), vec![arg], Ty::Int);
    b.set_return(result);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("call @callee(%0)"));
    assert!(out.contains("return %1"));
}

// ── Builder + Display: intrinsic call ──────────────────────────

#[test]
fn test_intrinsic_call() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "printer");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    let msg = b.push_const(Constant::String("hello".into()), Ty::String);
    let result = b.push_call(CallTarget::Intrinsic("print".into()), vec![msg], Ty::Unit);
    b.set_return(result);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("call intrinsic:print(%0)"));
}

// ── Builder + Display: constants ───────────────────────────────

#[test]
fn test_constant_display() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "constants");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    let _int = b.push_const(Constant::Int(42), Ty::Int);
    let _float = b.push_const(Constant::Float(std::f64::consts::PI), Ty::Float);
    let _str = b.push_const(Constant::String("hello".into()), Ty::String);
    let _ch = b.push_const(Constant::Char('a'), Ty::Char);
    let _bool = b.push_const(Constant::Bool(true), Ty::Bool);
    let unit = b.push_const(Constant::Unit, Ty::Unit);
    b.set_return(unit);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("const 42 : Int"));
    assert!(out.contains("const 3.141592653589793 : Float"));
    assert!(out.contains(r#"const "hello" : String"#));
    assert!(out.contains("const 'a' : Char"));
    assert!(out.contains("const true : Bool"));
    assert!(out.contains("const () : Unit"));
}

// ── Module display ─────────────────────────────────────────────

#[test]
fn test_module_display() {
    let mut interner = mk_interner();
    let tree = ItemTree::default();

    let fn1_name = mk_name(&mut interner, "foo");
    let fn2_name = mk_name(&mut interner, "bar");
    let entry_label = mk_name(&mut interner, "entry");

    // Function 1
    let mut b1 = KirBuilder::new();
    let e1 = b1.new_block(Some(entry_label));
    b1.switch_to(e1);
    let v = b1.push_const(Constant::Unit, Ty::Unit);
    b1.set_return(v);
    let f1 = b1.build(
        fn1_name,
        vec![],
        Ty::Unit,
        EffectSet::default(),
        e1,
        KirContracts::default(),
    );

    // Function 2
    let entry_label2 = mk_name(&mut interner, "entry");
    let mut b2 = KirBuilder::new();
    let e2 = b2.new_block(Some(entry_label2));
    b2.switch_to(e2);
    let v = b2.push_const(Constant::Int(0), Ty::Int);
    b2.set_return(v);
    let f2 = b2.build(
        fn2_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        e2,
        KirContracts::default(),
    );

    let mut module = KirModule::new();
    let id1 = module.functions.alloc(f1);
    let _id2 = module.functions.alloc(f2);
    module.entry = Some(id1);

    let ctx = DisplayCtx::new(&interner, &tree);
    let text = display_module(&module, &ctx);

    assert!(text.contains("fn @foo()"));
    assert!(text.contains("fn @bar()"));
}

// ── Validator: valid function passes ───────────────────────────

#[test]
fn test_validator_valid_function() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "valid");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);
    let v = b.push_const(Constant::Int(1), Ty::Int);
    b.set_return(v);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(diags.is_empty(), "expected no diagnostics: {:?}", diags);
}

// ── Validator: missing terminator ──────────────────────────────

#[test]
fn test_validator_missing_terminator() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "no_term");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);
    let _v = b.push_const(Constant::Int(1), Ty::Int);
    // No terminator set!

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("no terminator"));
}

// ── Validator: entry block has parameters ──────────────────────

#[test]
fn test_validator_entry_block_params() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "bad_entry");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));

    // Add a parameter to the entry block (not allowed)
    let _param = b.add_block_param(entry, None, Ty::Int);
    b.switch_to(entry);
    let v = b.push_const(Constant::Int(0), Ty::Int);
    b.set_return(v);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("entry block must have zero parameters")),
        "expected entry param diagnostic: {:?}",
        diags
    );
}

// ── Validator: return type mismatch ────────────────────────────

#[test]
fn test_validator_return_type_mismatch() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "bad_ret");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);
    // Function says it returns Int, but we return a Bool
    let v = b.push_const(Constant::Bool(true), Ty::Bool);
    b.set_return(v);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("return type mismatch")),
        "expected return type mismatch diagnostic: {:?}",
        diags
    );
}

// ── Validator: branch argument count mismatch ──────────────────

#[test]
fn test_validator_branch_arg_count_mismatch() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "bad_args");
    let entry_label = mk_name(&mut interner, "entry");
    let target_label = mk_name(&mut interner, "target");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    let target = b.new_block(Some(target_label));

    // Target expects 1 param, but we pass 0 args
    let _param = b.add_block_param(target, None, Ty::Int);
    b.switch_to(target);
    let v = b.push_const(Constant::Int(0), Ty::Int);
    b.set_return(v);

    b.switch_to(entry);
    b.set_jump(BranchTarget {
        block: target,
        args: vec![], // mismatch: should be 1
    });

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d
            .message
            .contains("passes 0 args but target block expects 1 params")),
        "expected arg count mismatch diagnostic: {:?}",
        diags
    );
}

// ── Validator: branch argument type mismatch ───────────────────

#[test]
fn test_validator_branch_arg_type_mismatch() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "bad_arg_ty");
    let entry_label = mk_name(&mut interner, "entry");
    let target_label = mk_name(&mut interner, "target");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    let target = b.new_block(Some(target_label));

    // Target expects Int param
    let param = b.add_block_param(target, None, Ty::Int);
    b.switch_to(target);
    b.set_return(param);

    // Entry jumps to target with a Bool arg
    b.switch_to(entry);
    let wrong_val = b.push_const(Constant::Bool(false), Ty::Bool);
    b.set_jump(BranchTarget {
        block: target,
        args: vec![wrong_val],
    });

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("argument type mismatch")),
        "expected arg type mismatch diagnostic: {:?}",
        diags
    );
}

// ── Switch terminator display ──────────────────────────────────

#[test]
fn test_switch_display() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "matcher");
    let entry_label = mk_name(&mut interner, "entry");
    let a_label = mk_name(&mut interner, "case_a");
    let b_label = mk_name(&mut interner, "case_b");
    let some_name = mk_name(&mut interner, "Some");
    let none_name = mk_name(&mut interner, "None");

    let mut builder = KirBuilder::new();
    let entry = builder.new_block(Some(entry_label));
    let case_a = builder.new_block(Some(a_label));
    let case_b = builder.new_block(Some(b_label));

    builder.switch_to(entry);
    let scrutinee = builder.push_const(Constant::Int(0), Ty::Int);
    builder.set_terminator(Terminator::Switch {
        scrutinee,
        cases: vec![
            kyokara_kir::block::SwitchCase {
                variant: some_name,
                target: BranchTarget {
                    block: case_a,
                    args: vec![],
                },
            },
            kyokara_kir::block::SwitchCase {
                variant: none_name,
                target: BranchTarget {
                    block: case_b,
                    args: vec![],
                },
            },
        ],
        default: None,
    });

    builder.switch_to(case_a);
    let v = builder.push_const(Constant::Int(1), Ty::Int);
    builder.set_return(v);

    builder.switch_to(case_b);
    let v = builder.push_const(Constant::Int(0), Ty::Int);
    builder.set_return(v);

    let func = builder.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("switch %0 {"));
    assert!(out.contains("Some -> case_a()"));
    assert!(out.contains("None -> case_b()"));
}

// ── Record operations display ──────────────────────────────────

#[test]
fn test_record_operations() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "records");
    let entry_label = mk_name(&mut interner, "entry");
    let x_field = mk_name(&mut interner, "x");
    let y_field = mk_name(&mut interner, "y");

    let record_ty = Ty::Record {
        fields: vec![(x_field, Ty::Int), (y_field, Ty::Int)],
    };

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    let v1 = b.push_const(Constant::Int(1), Ty::Int);
    let v2 = b.push_const(Constant::Int(2), Ty::Int);
    let rec = b.push_record_create(vec![(x_field, v1), (y_field, v2)], record_ty.clone());
    let field = b.push_field_get(rec, x_field, Ty::Int);
    let v3 = b.push_const(Constant::Int(10), Ty::Int);
    let updated = b.push_record_update(rec, vec![(x_field, v3)], record_ty);
    let _ = updated;
    b.set_return(field);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("record_create { x: %0, y: %1 }"));
    assert!(out.contains("field_get %2, x"));
    assert!(out.contains("record_update %2, { x: %4 }"));
}

// ── Assert instruction display ─────────────────────────────────

#[test]
fn test_assert_display() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "checked");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    let cond = b.push_const(Constant::Bool(true), Ty::Bool);
    let _assertion = b.push_assert(cond, "x must be positive".to_string(), Ty::Unit);
    let v = b.push_const(Constant::Unit, Ty::Unit);
    b.set_return(v);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains(r#"assert %0, "x must be positive""#));
}

// ── Hole instruction display ───────────────────────────────────

#[test]
fn test_hole_display() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "with_hole");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);

    let hole = b.push_hole(0, vec![], Ty::Int);
    b.set_return(hole);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("hole #0 : Int"));
}

// ── Unreachable terminator ─────────────────────────────────────

#[test]
fn test_unreachable_terminator() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "diverge");
    let entry_label = mk_name(&mut interner, "entry");

    let mut b = KirBuilder::new();
    let entry = b.new_block(Some(entry_label));
    b.switch_to(entry);
    b.set_unreachable();

    let func = b.build(
        fn_name,
        vec![],
        Ty::Never,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("unreachable"));

    let diags = validate_function(&func, &interner);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
}

// ── Unlabeled blocks use bb0, bb1, etc. ────────────────────────

#[test]
fn test_unlabeled_blocks() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "nolabel");

    let mut b = KirBuilder::new();
    let entry = b.new_block(None);
    b.switch_to(entry);
    let v = b.push_const(Constant::Unit, Ty::Unit);
    b.set_return(v);

    let func = b.build(
        fn_name,
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let tree = ItemTree::default();
    let ctx = DisplayCtx::new(&interner, &tree);
    let mut out = String::new();
    display_function(&func, &ctx, &mut out).unwrap();

    assert!(out.contains("bb0:"), "expected bb0 label in: {}", out);
}

// ── Validator: invalid entry block ─────────────────────────────

#[test]
fn test_validator_invalid_entry_block_does_not_panic() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "bad_func");

    let mut b = KirBuilder::new();
    let entry = b.new_block(None);
    b.switch_to(entry);
    let v = b.push_const(Constant::Int(0), Ty::Int);
    b.set_return(v);

    // Build with a bogus entry_block that doesn't exist in the arena.
    let bogus_entry: kyokara_kir::block::BlockId = Idx::from_raw(la_arena::RawIdx::from_u32(999));
    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        bogus_entry,
        KirContracts::default(),
    );

    // Should NOT panic, should return a diagnostic.
    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("entry block")),
        "expected 'invalid entry block' diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn test_validator_invalid_contract_value_ids() {
    let mut interner = mk_interner();
    let fn_name = mk_name(&mut interner, "contract_func");

    let mut b = KirBuilder::new();
    let entry = b.new_block(None);
    b.switch_to(entry);
    let v = b.push_const(Constant::Int(0), Ty::Int);
    b.set_return(v);

    // Build with bogus contract ValueIds.
    let bogus_value: kyokara_kir::value::ValueId = Idx::from_raw(la_arena::RawIdx::from_u32(999));
    let contracts = KirContracts {
        requires: vec![bogus_value],
        ensures: vec![bogus_value],
    };
    let func = b.build(
        fn_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        contracts,
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("requires")),
        "expected diagnostic for invalid requires ValueId, got: {:?}",
        diags
    );
    assert!(
        diags.iter().any(|d| d.message.contains("ensures")),
        "expected diagnostic for invalid ensures ValueId, got: {:?}",
        diags
    );
}
