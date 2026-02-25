//! `kyokara-eval` — Tree-walking interpreter for Kyokara.
//!
//! Walks the HIR expression trees produced by the compiler frontend
//! and evaluates them directly. Used by `kyokara run <file>`.

pub mod env;
pub mod error;
pub mod interpreter;
pub mod intrinsics;
pub mod value;

use kyokara_hir::{
    FnItem, FnParam, Name, Path, TypeRef, check_module, collect_item_tree, register_builtin_types,
};
use kyokara_intern::Interner;
use kyokara_span::FileId;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

use crate::error::RuntimeError;
use crate::interpreter::Interpreter;
use crate::value::Value;

/// Result of running a Kyokara program.
pub struct RunResult {
    pub value: Value,
    pub interner: Interner,
}

/// Parse, type-check, and evaluate a Kyokara source file.
///
/// Injects builtin types (`Option`, `Result`) and intrinsic function
/// signatures before type-checking so that constructors and calls to
/// `println`, `int_to_string`, etc. resolve correctly.
pub fn run(source: &str) -> Result<RunResult, RuntimeError> {
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);
    if !parse.errors.is_empty() {
        let msgs: Vec<String> = parse.errors.iter().map(|e| format!("{e:?}")).collect();
        return Err(RuntimeError::TypeError(format!(
            "parse errors: {}",
            msgs.join("; ")
        )));
    }

    // 2. Build CST.
    let root = SyntaxNode::new_root(parse.green);
    let sf = SourceFile::cast(root.clone()).expect("parsed root should cast to SourceFile");

    // 3. Collect item tree.
    let mut interner = Interner::new();
    let mut item_result = collect_item_tree(&sf, file_id, &mut interner);

    if item_result
        .diagnostics
        .iter()
        .any(|d| d.severity == kyokara_diagnostics::Severity::Error)
    {
        let msgs: Vec<String> = item_result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(RuntimeError::TypeError(format!(
            "lowering errors: {}",
            msgs.join("; ")
        )));
    }

    // 4. Register builtin types (Option, Result) before intrinsics and type-checking.
    register_builtin_types(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 5. Register intrinsic function signatures in the item tree + module scope.
    register_intrinsics(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 6. Type-check.
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        file_id,
        &mut interner,
    );

    // Check raw_diagnostics for real type errors.
    // (type_check.diagnostics also includes body-lowering false positives
    // for constructor pattern bindings, so we skip those.)
    if !type_check.raw_diagnostics.is_empty() {
        let msgs: Vec<String> = type_check
            .raw_diagnostics
            .iter()
            .map(|(data, _span)| {
                data.clone()
                    .into_diagnostic(
                        kyokara_span::Span {
                            file: file_id,
                            range: Default::default(),
                        },
                        &interner,
                        &item_result.tree,
                    )
                    .message
            })
            .collect();
        return Err(RuntimeError::TypeError(msgs.join("; ")));
    }

    // 7. Interpret.
    let mut interp = Interpreter::new(
        item_result.tree,
        item_result.module_scope,
        type_check.fn_bodies,
        interner,
    );

    let value = interp.run_main()?;
    let interner = interp.into_interner();
    Ok(RunResult { value, interner })
}

/// Register intrinsic functions as bodyless items in the item tree and module scope.
fn register_intrinsics(
    tree: &mut kyokara_hir::ItemTree,
    scope: &mut kyokara_hir::ModuleScope,
    interner: &mut Interner,
) {
    let intrinsic_sigs = intrinsic_signatures(interner);

    for (name, fn_item) in intrinsic_sigs {
        let idx = tree.functions.alloc(fn_item);
        scope.functions.insert(name, idx);
    }
}

/// Build FnItem signatures for each intrinsic.
fn intrinsic_signatures(interner: &mut Interner) -> Vec<(Name, FnItem)> {
    let string_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "String")),
        args: Vec::new(),
    };
    let int_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Int")),
        args: Vec::new(),
    };
    let unit_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Unit")),
        args: Vec::new(),
    };

    vec![
        // print(s: String) -> Unit
        (
            Name::new(interner, "print"),
            FnItem {
                name: Name::new(interner, "print"),
                type_params: Vec::new(),
                params: vec![FnParam {
                    name: Name::new(interner, "s"),
                    ty: string_ty.clone(),
                }],
                ret_type: Some(unit_ty.clone()),
                with_caps: Vec::new(),
                pipe_caps: Vec::new(),
                has_body: false,
            },
        ),
        // println(s: String) -> Unit
        (
            Name::new(interner, "println"),
            FnItem {
                name: Name::new(interner, "println"),
                type_params: Vec::new(),
                params: vec![FnParam {
                    name: Name::new(interner, "s"),
                    ty: string_ty.clone(),
                }],
                ret_type: Some(unit_ty.clone()),
                with_caps: Vec::new(),
                pipe_caps: Vec::new(),
                has_body: false,
            },
        ),
        // int_to_string(n: Int) -> String
        (
            Name::new(interner, "int_to_string"),
            FnItem {
                name: Name::new(interner, "int_to_string"),
                type_params: Vec::new(),
                params: vec![FnParam {
                    name: Name::new(interner, "n"),
                    ty: int_ty.clone(),
                }],
                ret_type: Some(string_ty.clone()),
                with_caps: Vec::new(),
                pipe_caps: Vec::new(),
                has_body: false,
            },
        ),
        // string_concat(a: String, b: String) -> String
        (
            Name::new(interner, "string_concat"),
            FnItem {
                name: Name::new(interner, "string_concat"),
                type_params: Vec::new(),
                params: vec![
                    FnParam {
                        name: Name::new(interner, "a"),
                        ty: string_ty.clone(),
                    },
                    FnParam {
                        name: Name::new(interner, "b"),
                        ty: string_ty.clone(),
                    },
                ],
                ret_type: Some(string_ty.clone()),
                with_caps: Vec::new(),
                pipe_caps: Vec::new(),
                has_body: false,
            },
        ),
    ]
}
