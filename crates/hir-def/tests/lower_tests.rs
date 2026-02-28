//! End-to-end tests: parse → collect item tree → lower body → verify HIR.
#![allow(clippy::unwrap_used)]

use kyokara_hir_def::body::lower::lower_body;
use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, Literal, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::TypeDefKind;
use kyokara_hir_def::item_tree::lower::collect_item_tree;
use kyokara_hir_def::pat::Pat;
use kyokara_intern::Interner;
use kyokara_span::FileId;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{FnDef, SourceFile};
use kyokara_syntax::ast::traits::HasName;

fn file_id() -> FileId {
    FileId(0)
}

/// Parse source, build CST, return SourceFile wrapper.
fn parse_source(src: &str) -> SyntaxNode {
    let parse = kyokara_syntax::parse(src);
    SyntaxNode::new_root(parse.green)
}

// ── Item tree tests ────────────────────────────────────────────────

#[test]
fn collect_fn_item() {
    let root = parse_source("fn add(x: Int, y: Int) -> Int { x }");
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert_eq!(result.tree.functions.len(), 1);
    let f = &result.tree.functions[result.tree.functions.iter().next().unwrap().0];
    assert_eq!(f.name.resolve(&interner), "add");
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].name.resolve(&interner), "x");
    assert_eq!(f.params[1].name.resolve(&interner), "y");
    assert!(f.ret_type.is_some());
    assert!(f.has_body);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn collect_type_alias() {
    let root = parse_source("type Age = Int");
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert_eq!(result.tree.types.len(), 1);
    let t = &result.tree.types[result.tree.types.iter().next().unwrap().0];
    assert_eq!(t.name.resolve(&interner), "Age");
    assert!(matches!(t.kind, TypeDefKind::Alias(_)));
}

#[test]
fn collect_adt_with_constructors() {
    let root = parse_source("type Option<T> = | Some(T) | None");
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert_eq!(result.tree.types.len(), 1);
    let t = &result.tree.types[result.tree.types.iter().next().unwrap().0];
    assert_eq!(t.name.resolve(&interner), "Option");
    assert_eq!(t.type_params.len(), 1);
    if let TypeDefKind::Adt { variants } = &t.kind {
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name.resolve(&interner), "Some");
        assert_eq!(variants[0].fields.len(), 1);
        assert_eq!(variants[1].name.resolve(&interner), "None");
        assert_eq!(variants[1].fields.len(), 0);
    } else {
        panic!("expected ADT");
    }

    // Constructors registered in module scope
    assert!(result.module_scope.constructors.len() >= 2);
}

#[test]
fn collect_module_and_imports() {
    let root = parse_source("module Foo\nimport Bar.Baz\nimport Qux as Q");
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert!(result.tree.module_name.is_some());
    assert_eq!(result.tree.imports.len(), 2);
    assert!(result.tree.imports[1].alias.is_some());
}

#[test]
fn duplicate_fn_diagnostic() {
    let root = parse_source("fn foo() { 1 }\nfn foo() { 2 }");
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert!(!result.diagnostics.is_empty());
    assert!(result.diagnostics[0].message.contains("duplicate function"));
}

#[test]
fn duplicate_type_diagnostic() {
    let root = parse_source("type Foo = Int\ntype Foo = Bool");
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert!(!result.diagnostics.is_empty());
    assert!(result.diagnostics[0].message.contains("duplicate type"));
}

// ── Body lowering tests ────────────────────────────────────────────

fn lower_fn_body(
    src: &str,
) -> (
    kyokara_hir_def::body::Body,
    Vec<kyokara_diagnostics::Diagnostic>,
    Interner,
) {
    lower_named_fn_body(src, None)
}

fn lower_named_fn_body(
    src: &str,
    name: Option<&str>,
) -> (
    kyokara_hir_def::body::Body,
    Vec<kyokara_diagnostics::Diagnostic>,
    Interner,
) {
    let root = parse_source(src);
    let sf = SourceFile::cast(root.clone()).unwrap();
    let mut interner = Interner::new();
    let item_result = collect_item_tree(&sf, file_id(), &mut interner);

    // Find the FnDef by name, or the last one if no name given.
    let fn_def = if let Some(target) = name {
        root.descendants()
            .filter_map(FnDef::cast)
            .find(|f: &FnDef| f.name_token().map(|t| t.text() == target).unwrap_or(false))
            .expect("named FnDef not found")
    } else {
        // Use the last FnDef (typically the one under test in multi-fn sources)
        root.descendants()
            .filter_map(FnDef::cast)
            .last()
            .expect("no FnDef found")
    };

    let body_result = lower_body(&fn_def, &item_result.module_scope, file_id(), &mut interner);
    (body_result.body, body_result.diagnostics, interner)
}

#[test]
fn lower_literal_expr() {
    let (body, diags, _) = lower_fn_body("fn foo() { 42 }");
    assert!(diags.is_empty());
    // Root should be a Block containing literal 42
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => {
            assert!(matches!(
                &body.exprs[*tail],
                Expr::Literal(Literal::Int(42))
            ));
        }
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_binary_expr() {
    let (body, diags, _) = lower_fn_body("fn foo(x: Int, y: Int) { x + y }");
    assert!(diags.is_empty());
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Binary {
                op: BinaryOp::Add, ..
            } => {}
            other => panic!("expected Binary Add, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_unary_expr() {
    let (body, diags, _) = lower_fn_body("fn foo(x: Bool) { !x }");
    assert!(diags.is_empty());
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Unary {
                op: UnaryOp::Not, ..
            } => {}
            other => panic!("expected Unary Not, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_if_expr() {
    let (body, diags, _) = lower_fn_body("fn foo(x: Bool) { if x { 1 } else { 2 } }");
    assert!(diags.is_empty());
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::If {
                else_branch: Some(_),
                ..
            } => {}
            other => panic!("expected If with else, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_match_expr() {
    let src = r#"
type Bool = | True | False
fn foo(x: Bool) {
    match x {
        True => 1
        False => 0
    }
}
"#;
    let (body, _, _) = lower_fn_body(src);
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
            }
            other => panic!("expected Match, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_let_binding() {
    let (body, diags, _) = lower_fn_body("fn foo() { let x = 42\n x }");
    assert!(diags.is_empty());
    match &body.exprs[body.root] {
        Expr::Block { stmts, tail, .. } => {
            assert_eq!(stmts.len(), 1);
            assert!(matches!(&stmts[0], Stmt::Let { .. }));
            assert!(tail.is_some());
        }
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_lambda_expr() {
    let (body, diags, _) = lower_fn_body("fn foo() { fn(x: Int) => x }");
    assert!(diags.is_empty());
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Lambda { params, .. } => {
                assert_eq!(params.len(), 1);
            }
            other => panic!("expected Lambda, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_return_expr() {
    let (body, diags, _) = lower_fn_body("fn foo() { return 42 }");
    assert!(diags.is_empty());
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Return(Some(_)) => {}
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_call_expr() {
    let (body, _, _) = lower_fn_body("fn bar() { 0 }\nfn foo() { bar() }");
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Call { args, .. } => {
                assert_eq!(args.len(), 0);
            }
            other => panic!("expected Call, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_field_expr() {
    // `r.x` in Kyokara is parsed as a dotted Path, not a FieldExpr.
    // FieldExpr only occurs on postfix `.` after a non-path expression.
    // Use a call result to get a true FieldExpr: `foo().x`.
    let (body, _, _) = lower_fn_body("fn foo() { foo().x }");
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Field { field, .. } => {
                let _ = field;
            }
            other => panic!("expected Field, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

// ── Desugaring tests ───────────────────────────────────────────────

#[test]
fn desugar_pipeline_to_call() {
    // x |> f(a) → Call { callee: f, args: [x, a] }
    let (body, _, interner) =
        lower_fn_body("fn f(x: Int, y: Int) { 0 }\nfn foo(x: Int) { x |> f(1) }");
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => {
            match &body.exprs[*tail] {
                Expr::Call { args, callee } => {
                    // Should have 2 args: x (piped) and 1
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[0], CallArg::Positional(_)));
                    // callee should be f
                    match &body.exprs[*callee] {
                        Expr::Path(p) => {
                            assert_eq!(p.segments[0].resolve(&interner), "f");
                        }
                        other => panic!("expected Path callee, got {other:?}"),
                    }
                }
                other => panic!("expected Call (desugared pipeline), got {other:?}"),
            }
        }
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn desugar_pipeline_bare_fn() {
    // x |> f → Call { callee: f, args: [x] }
    let (body, _, _) = lower_fn_body("fn f(x: Int) { 0 }\nfn foo(x: Int) { x |> f }");
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::Call { args, .. } => {
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call (desugared bare pipeline), got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn desugar_propagation_to_match() {
    // e? → Match with Ok/Err arms
    let (body, _, interner) =
        lower_fn_body("type Result<T> = | Ok(T) | Err(T)\nfn foo(e: Result<Int>) { e? }");
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => {
            match &body.exprs[*tail] {
                Expr::Match { arms, .. } => {
                    assert_eq!(arms.len(), 2);
                    // First arm: Ok pattern
                    match &body.pats[arms[0].pat] {
                        Pat::Constructor { path, args } => {
                            assert_eq!(path.segments[0].resolve(&interner), "Ok");
                            assert_eq!(args.len(), 1);
                        }
                        other => panic!("expected Ok constructor pat, got {other:?}"),
                    }
                    // Second arm: Err pattern with Return body
                    match &body.pats[arms[1].pat] {
                        Pat::Constructor { path, .. } => {
                            assert_eq!(path.segments[0].resolve(&interner), "Err");
                        }
                        other => panic!("expected Err constructor pat, got {other:?}"),
                    }
                    // Err body should be Return(Call(Err, [__e]))
                    match &body.exprs[arms[1].body] {
                        Expr::Return(Some(call_idx)) => match &body.exprs[*call_idx] {
                            Expr::Call { args, .. } => {
                                assert_eq!(args.len(), 1);
                            }
                            other => panic!("expected Call in Err return, got {other:?}"),
                        },
                        other => panic!("expected Return in Err arm, got {other:?}"),
                    }
                }
                other => panic!("expected Match (desugared propagation), got {other:?}"),
            }
        }
        other => panic!("expected Block, got {other:?}"),
    }
}

// ── Scope tests ────────────────────────────────────────────────────

#[test]
fn pat_bind_introduces_scope_entry() {
    let (body, diags, _) = lower_fn_body("fn foo() { let x = 1\n x }");
    assert!(diags.is_empty());
    // x should be in scope — no unresolved name diagnostic
    assert!(!body.pat_scopes.is_empty());
}

#[test]
fn match_arm_introduces_scope() {
    let src = r#"
type Option<T> = | Some(T) | None
fn foo(x: Option<Int>) {
    match x {
        Some(v) => v
        None => 0
    }
}
"#;
    let (body, _, _) = lower_fn_body(src);
    // v should be bound in the Some arm — check pat_scopes
    let has_v = body
        .pat_scopes
        .iter()
        .any(|(pat_idx, _)| matches!(&body.pats[*pat_idx], Pat::Bind { .. }));
    assert!(has_v);
}

#[test]
fn lambda_params_in_scope() {
    let (_body, diags, _) = lower_fn_body("fn foo() { fn(x: Int) => x }");
    assert!(diags.is_empty());
    // x should be resolvable in the lambda body
}

// ── Diagnostic tests ───────────────────────────────────────────────

#[test]
fn unresolved_name_diagnostic() {
    let (_, diags, _) = lower_fn_body("fn foo() { unknown_var }");
    assert!(!diags.is_empty());
    assert!(diags[0].message.contains("unresolved name"));
}

#[test]
fn resolved_param_no_diagnostic() {
    let (_, diags, _) = lower_fn_body("fn foo(x: Int) { x }");
    assert!(diags.is_empty(), "params should be resolved: {diags:?}");
}

#[test]
fn resolved_fn_no_diagnostic() {
    let (_, diags, _) = lower_fn_body("fn bar() { 0 }\nfn foo() { bar() }");
    // bar should resolve to the function item
    let unresolved: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("unresolved"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "bar should be resolved: {unresolved:?}"
    );
}

// ── Record and old expression tests ────────────────────────────────

#[test]
fn lower_record_literal() {
    let src = r#"
type Point = { x: Int, y: Int }
fn foo() { Point { x: 1, y: 2 } }
"#;
    let (body, _, _) = lower_fn_body(src);
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => match &body.exprs[*tail] {
            Expr::RecordLit { fields, .. } => {
                assert_eq!(fields.len(), 2);
            }
            other => panic!("expected RecordLit, got {other:?}"),
        },
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn lower_old_expr() {
    let (body, _, _) = lower_fn_body("fn foo(x: Int) ensures old(x) == x { x }");
    // ensures clause should contain Old expr
    assert!(body.ensures.is_some());
}

#[test]
fn lower_hole_expr() {
    let (body, _, _) = lower_fn_body("fn foo() { _ }");
    match &body.exprs[body.root] {
        Expr::Block {
            tail: Some(tail), ..
        } => {
            assert!(matches!(&body.exprs[*tail], Expr::Hole));
        }
        other => panic!("expected Block, got {other:?}"),
    }
}

// ── Cap and property item tree tests ───────────────────────────────

#[test]
fn collect_cap_def() {
    let src = r#"
cap Console {
    fn print(msg: String) -> Unit {}
    fn read() -> String {}
}
"#;
    let root = parse_source(src);
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert_eq!(result.tree.caps.len(), 1);
    let cap = &result.tree.caps[result.tree.caps.iter().next().unwrap().0];
    assert_eq!(cap.name.resolve(&interner), "Console");
    assert_eq!(cap.functions.len(), 2);
}

#[test]
fn collect_property_def() {
    let src = "property commutative(x: Int, y: Int) { x + y == y + x }";
    let root = parse_source(src);
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    assert_eq!(result.tree.properties.len(), 1);
}

#[test]
fn collect_with_clause() {
    let src = "fn foo() -> Int with Console { 0 }";
    let root = parse_source(src);
    let sf = SourceFile::cast(root).unwrap();
    let mut interner = Interner::new();
    let result = collect_item_tree(&sf, file_id(), &mut interner);

    let f = &result.tree.functions[result.tree.functions.iter().next().unwrap().0];
    assert_eq!(f.with_caps.len(), 1);
}
