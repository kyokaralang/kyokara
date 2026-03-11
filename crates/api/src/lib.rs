//! `kyokara-api` — Compiler-as-API with JSON serialization.
//!
//! This crate owns all serialization. Internal compiler types stay
//! `serde`-free; instead, `api` defines its own DTO types that mirror
//! the internal structures and derive `Serialize`.
//!
//! Outputs (v0.0+):
//! - `CheckOutput` with structured diagnostics, typed hole specs, symbol graph,
//!   and optional typed AST (opt-in via `CheckOptions`)

use kyokara_diagnostics::Severity;
use kyokara_hir::{
    CheckResult, HoleInfo, TyDiagnosticData, TypeDefKind, TypeRef, display_ty_with_tree,
};
use kyokara_hir_def::expr::{CallArg, Expr, ExprIdx, MatchArm, PatIdx, Stmt};
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree};
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::{CoreType, ModuleScope, ResolvedName};
use kyokara_hir_def::scope::ScopeDef;
use kyokara_hir_ty::TypeCheckResult;
use kyokara_intern::Interner;
use kyokara_span::TextRange;
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{MatchExpr as SyntaxMatchExpr, Pat as SyntaxPat};
use serde::Serialize;

// ── Top-level output ────────────────────────────────────────────────

/// Options for `check`/`check_project` API calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct CheckOptions {
    pub include_typed_ast: bool,
}

/// Top-level output from the check pipeline.
#[derive(Debug, Serialize)]
pub struct CheckOutput {
    pub diagnostics: Vec<DiagnosticDto>,
    pub holes: Vec<HoleSpecDto>,
    pub symbol_graph: SymbolGraphDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed_ast: Option<TypedAstDto>,
}

// ── Diagnostic DTOs ─────────────────────────────────────────────────

/// A serialisable diagnostic for JSON output.
#[derive(Debug, Serialize)]
pub struct DiagnosticDto {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub span: SpanDto,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
    pub fixes: Vec<FixDto>,
}

/// A suggested fix.
#[derive(Debug, Serialize)]
pub struct FixDto {
    pub message: String,
    pub span: SpanDto,
    pub replacement: String,
}

/// Source span for JSON output.
#[derive(Debug, Serialize)]
pub struct SpanDto {
    pub file: String,
    pub start: u32,
    pub end: u32,
}

// ── Hole spec DTOs ──────────────────────────────────────────────────

/// A typed hole specification.
#[derive(Debug, Serialize)]
pub struct HoleSpecDto {
    pub id: usize,
    pub name: Option<String>,
    pub expected_type: Option<String>,
    pub effects: Vec<String>,
    pub inputs: Vec<InputVarDto>,
    pub span: SpanDto,
}

/// An available local variable at a hole site.
#[derive(Debug, Serialize)]
pub struct InputVarDto {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

// ── Symbol graph DTOs ───────────────────────────────────────────────

/// Structured map of definitions and call relationships in a module.
#[derive(Debug, Serialize)]
pub struct SymbolGraphDto {
    pub functions: Vec<FnNodeDto>,
    pub types: Vec<TypeNodeDto>,
    pub capabilities: Vec<CapNodeDto>,
    /// `true` when the source has parse errors, indicating the graph
    /// was built from a recovered/partial CST and may contain artifacts.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

/// A function node in the symbol graph.
#[derive(Debug, Serialize)]
pub struct FnNodeDto {
    pub id: String,
    pub name: String,
    pub params: Vec<ParamDto>,
    pub return_type: Option<String>,
    pub effects: Vec<String>,
    /// Unique callee function IDs (set semantics; no duplicates per caller).
    pub calls: Vec<String>,
}

/// A parameter in the symbol graph.
#[derive(Debug, Serialize)]
pub struct ParamDto {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// A type node in the symbol graph.
#[derive(Debug, Serialize)]
pub struct TypeNodeDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub type_params: Vec<String>,
    pub fields: Vec<ParamDto>,
    pub variants: Vec<VariantDto>,
}

/// A variant of an ADT in the symbol graph.
#[derive(Debug, Serialize)]
pub struct VariantDto {
    pub id: String,
    pub name: String,
    pub fields: Vec<String>,
}

/// A capability node in the symbol graph.
#[derive(Debug, Serialize)]
pub struct CapNodeDto {
    pub id: String,
    pub name: String,
    pub functions: Vec<String>,
}

// ── Typed AST DTOs ──────────────────────────────────────────────────

/// Typed AST payload (opt-in in `CheckOptions`).
#[derive(Debug, Serialize)]
pub struct TypedAstDto {
    pub partial: bool,
    pub files: Vec<TypedAstFileDto>,
}

/// Typed AST for a single file/module.
#[derive(Debug, Serialize)]
pub struct TypedAstFileDto {
    pub file: String,
    pub functions: Vec<TypedFnAstDto>,
}

/// Typed AST for a single lowered function body.
#[derive(Debug, Serialize)]
pub struct TypedFnAstDto {
    pub id: String,
    pub name: String,
    pub root_expr: u32,
    pub expr_nodes: Vec<TypedExprNodeDto>,
    pub pat_nodes: Vec<TypedPatNodeDto>,
}

/// A typed expression node.
#[derive(Debug, Serialize)]
pub struct TypedExprNodeDto {
    pub id: u32,
    pub kind: String,
    pub span: SpanDto,
    pub ty: String,
    pub expr_refs: Vec<u32>,
    pub pat_refs: Vec<u32>,
    pub symbol: Option<String>,
}

/// A typed pattern node.
#[derive(Debug, Serialize)]
pub struct TypedPatNodeDto {
    pub id: u32,
    pub kind: String,
    pub span: SpanDto,
    pub ty: String,
    pub pat_refs: Vec<u32>,
    pub symbol: Option<String>,
}

// ── Public entry point ──────────────────────────────────────────────

/// Run the full check pipeline on source text and return structured output.
pub fn check(source: &str, file_name: &str) -> CheckOutput {
    check_with_options(source, file_name, &CheckOptions::default())
}

/// Run the full check pipeline with explicit options.
pub fn check_with_options(source: &str, file_name: &str, options: &CheckOptions) -> CheckOutput {
    let result = kyokara_hir::check_file(source);
    convert_result(&result, file_name, options)
}

/// Run the check pipeline on a multi-file project and return structured output.
pub fn check_project(entry_file: &std::path::Path) -> CheckOutput {
    check_project_with_options(entry_file, &CheckOptions::default())
}

/// Run the check pipeline on a multi-file project with explicit options.
pub fn check_project_with_options(
    entry_file: &std::path::Path,
    options: &CheckOptions,
) -> CheckOutput {
    let result = kyokara_hir::check_project(entry_file);
    let mut diagnostics = Vec::new();
    let interner = &result.interner;
    let mut typed_ast_files = Vec::new();
    let mut typed_ast_partial = result.parse_errors.iter().any(|(_, errs)| !errs.is_empty());

    // Aggregate parse errors from all modules.
    for (mod_path, errors) in &result.parse_errors {
        let file_id = result.module_graph.get(mod_path).map(|i| i.file_id);
        let file_name = file_id
            .and_then(|fid| result.file_map.path(fid))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<unresolved:{:?}>", mod_path));

        for err in errors {
            diagnostics.push(DiagnosticDto {
                code: "E0100".into(),
                severity: "error".into(),
                message: err.message.clone(),
                span: SpanDto {
                    file: file_name.clone(),
                    start: err.range_start,
                    end: err.range_end,
                },
                expected_type: None,
                actual_type: None,
                fixes: Vec::new(),
            });
        }
    }

    // Lowering diagnostics.
    for diag in &result.lowering_diagnostics {
        let code = kyokara_hir::lowering_diagnostic_code(diag);
        let file_name = result
            .file_map
            .path(diag.span.file)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<unresolved:FileId({})>", diag.span.file.0));
        diagnostics.push(convert_lowering_diagnostic(diag, code, &file_name));
    }

    // Type-check diagnostics from all modules.
    for (mod_path, tc) in &result.type_checks {
        let mod_info = result
            .module_graph
            .get(mod_path)
            .expect("module graph entry exists while iterating type_checks");
        let file_name = result
            .file_map
            .path(mod_info.file_id)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<unresolved:{:?}>", mod_path));
        let root = SyntaxNode::new_root(kyokara_syntax::parse(&mod_info.source).green);
        let item_tree = &mod_info.item_tree;

        // Body lowering diagnostics (unresolved names, duplicates).
        for diag in &tc.body_lowering_diagnostics {
            let code = kyokara_hir::lowering_diagnostic_code(diag);
            diagnostics.push(convert_lowering_diagnostic(diag, code, &file_name));
        }

        for (data, span) in &tc.raw_diagnostics {
            diagnostics.push(convert_ty_diagnostic(
                data,
                span,
                interner,
                item_tree,
                &file_name,
                Some(&root),
            ));
        }
    }

    // Collect holes and symbol graphs from all modules.
    let mut holes = Vec::new();
    let mut all_functions = Vec::new();
    let mut all_types = Vec::new();
    let mut all_capabilities = Vec::new();

    let builtin_names: std::collections::HashSet<&str> = [
        "Option",
        "Result",
        "List",
        "BitSet",
        "Map",
        "Set",
        "Deque",
        "MutableList",
        "MutableDeque",
        "MutablePriorityQueue",
        "MutableMap",
        "MutableSet",
        "MutableBitSet",
        "Seq",
        "ParseError",
    ]
    .into_iter()
    .collect();
    let mut seen_builtins: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (mod_path, tc) in &result.type_checks {
        let file_name = result
            .module_graph
            .get(mod_path)
            .and_then(|i| result.file_map.path(i.file_id))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("<unresolved:{:?}>", mod_path));

        // Build module prefix from ModulePath segments.
        let prefix = if mod_path.is_root() {
            None
        } else {
            Some(
                mod_path
                    .0
                    .iter()
                    .map(|n| n.resolve(interner))
                    .collect::<Vec<_>>()
                    .join("."),
            )
        };

        if let Some(info) = result.module_graph.get(mod_path) {
            // Holes.
            for fn_result in tc.fn_results.values() {
                for hole in &fn_result.holes {
                    holes.push(convert_hole(
                        holes.len(),
                        hole,
                        interner,
                        &info.item_tree,
                        &file_name,
                    ));
                }
            }

            // Symbol graph with module prefix.
            let graph = build_module_symbol_graph(
                &info.item_tree,
                &info.scope,
                tc,
                interner,
                prefix.as_deref(),
            );
            all_functions.extend(graph.functions);
            all_capabilities.extend(graph.capabilities);

            if options.include_typed_ast {
                let (functions, partial) = build_typed_ast_for_module(
                    &info.item_tree,
                    &info.scope,
                    tc,
                    interner,
                    &file_name,
                    prefix.as_deref(),
                );
                typed_ast_partial |= partial;
                typed_ast_files.push(TypedAstFileDto {
                    file: file_name.clone(),
                    functions,
                });
            }

            // Deduplicate builtins: only keep the first copy.
            for t in graph.types {
                if builtin_names.contains(t.name.as_str()) {
                    if !seen_builtins.insert(t.name.clone()) {
                        continue; // skip duplicate
                    }
                    // Emit builtin with bare ID (no module prefix).
                    all_types.push(TypeNodeDto {
                        id: symbol_id("type", &t.name, None),
                        ..t
                    });
                } else {
                    all_types.push(t);
                }
            }
        } else if options.include_typed_ast {
            typed_ast_partial = true;
        }
    }

    // Post-process call edges:
    // 1) Keep edges that already point at emitted function IDs.
    // 2) For non-emitted IDs, rewrite by bare function name only when there is
    //    a unique global target.
    // 3) Drop unresolved/ambiguous edges instead of misattributing them.
    let emitted_fn_ids: std::collections::HashSet<String> =
        all_functions.iter().map(|f| f.id.clone()).collect();
    let mut fn_name_to_ids: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for f in &all_functions {
        fn_name_to_ids
            .entry(f.name.clone())
            .or_default()
            .push(f.id.clone());
    }
    for ids in fn_name_to_ids.values_mut() {
        ids.sort();
        ids.dedup();
    }

    #[derive(Clone, Debug)]
    struct CallRewriteRule {
        unique_target: Option<String>,
        preferred_qualified_for_bare: Option<String>,
    }

    let mut rewrite_rules: std::collections::HashMap<String, CallRewriteRule> =
        std::collections::HashMap::new();
    for (name, candidates) in &fn_name_to_ids {
        let unique_target = (candidates.len() == 1).then(|| candidates[0].clone());
        let mut qualified_iter = candidates.iter().filter(|id| {
            id.strip_prefix("fn::")
                .map(|rest| rest.contains("::"))
                .unwrap_or(false)
        });
        let preferred_qualified_for_bare = match (qualified_iter.next(), qualified_iter.next()) {
            (Some(id), None) => Some(id.clone()),
            _ => None,
        };
        rewrite_rules.insert(
            name.clone(),
            CallRewriteRule {
                unique_target,
                preferred_qualified_for_bare,
            },
        );
    }

    for func in &mut all_functions {
        let mut rewritten_calls = Vec::with_capacity(func.calls.len());
        for call in &func.calls {
            // Preserve exact resolved targets already present in the emitted graph.
            // This avoids rebinding local edges via name-only heuristics.
            if emitted_fn_ids.contains(call) {
                rewritten_calls.push(call.clone());
                continue;
            }

            // Extract bare callee name from IDs like `fn::foo` or `fn::m::foo`.
            let callee_name = call.strip_prefix("fn::").unwrap_or(call);
            let bare_name = callee_name.rsplit("::").next().unwrap_or(callee_name);

            let Some(rule) = rewrite_rules.get(bare_name) else {
                // Unknown callee in project graph.
                continue;
            };

            if let Some(target) = &rule.unique_target {
                rewritten_calls.push(target.clone());
                continue;
            }

            // If we have exactly one qualified target and the current edge is a
            // bare `fn::name` alias, prefer the qualified definition.
            let is_bare_fn_id = call
                .strip_prefix("fn::")
                .map(|rest| !rest.contains("::"))
                .unwrap_or(false);
            if is_bare_fn_id && let Some(target) = &rule.preferred_qualified_for_bare {
                rewritten_calls.push(target.clone());
                continue;
            }

            // No safe rewrite available: drop unresolved/ambiguous edge.
        }
        func.calls = rewritten_calls;
        dedupe_call_ids(&mut func.calls);
    }

    CheckOutput {
        diagnostics,
        holes,
        symbol_graph: SymbolGraphDto {
            functions: all_functions,
            types: all_types,
            capabilities: all_capabilities,
            partial: result.parse_errors.iter().any(|(_, errs)| !errs.is_empty()),
        },
        typed_ast: options.include_typed_ast.then_some(TypedAstDto {
            partial: typed_ast_partial,
            files: typed_ast_files,
        }),
    }
}

// ── Conversion helpers ──────────────────────────────────────────────

fn convert_result(result: &CheckResult, file_name: &str, options: &CheckOptions) -> CheckOutput {
    let interner = &result.interner;
    let mut diagnostics = Vec::new();

    // Parse errors → diagnostics with code E0100.
    for err in &result.parse_errors {
        diagnostics.push(DiagnosticDto {
            code: "E0100".into(),
            severity: "error".into(),
            message: err.message.clone(),
            span: SpanDto {
                file: file_name.into(),
                start: err.range_start,
                end: err.range_end,
            },
            expected_type: None,
            actual_type: None,
            fixes: Vec::new(),
        });
    }

    // Lowering diagnostics → E0101 (unresolved name) / E0102 (duplicate definition).
    for diag in &result.lowering_diagnostics {
        let code = kyokara_hir::lowering_diagnostic_code(diag);
        diagnostics.push(convert_lowering_diagnostic(diag, code, file_name));
    }

    // Body lowering diagnostics (unresolved names, duplicates from body lowering).
    for diag in &result.type_check.body_lowering_diagnostics {
        let code = kyokara_hir::lowering_diagnostic_code(diag);
        diagnostics.push(convert_lowering_diagnostic(diag, code, file_name));
    }

    // Type-checker raw diagnostics.
    let root = SyntaxNode::new_root(result.green.clone());
    for (data, span) in &result.type_check.raw_diagnostics {
        diagnostics.push(convert_ty_diagnostic(
            data,
            span,
            interner,
            &result.item_tree,
            file_name,
            Some(&root),
        ));
    }

    // Collect hole specs from all function results.
    let mut holes = Vec::new();
    for fn_result in result.type_check.fn_results.values() {
        for hole in &fn_result.holes {
            holes.push(convert_hole(
                holes.len(),
                hole,
                interner,
                &result.item_tree,
                file_name,
            ));
        }
    }

    // Build symbol graph.
    let mut symbol_graph = build_symbol_graph(result);
    for func in &mut symbol_graph.functions {
        dedupe_call_ids(&mut func.calls);
    }
    symbol_graph.partial = !result.parse_errors.is_empty();

    let typed_ast = if options.include_typed_ast {
        let (functions, partial) = build_typed_ast_for_module(
            &result.item_tree,
            &result.module_scope,
            &result.type_check,
            interner,
            file_name,
            None,
        );
        Some(TypedAstDto {
            partial: !result.parse_errors.is_empty() || partial,
            files: vec![TypedAstFileDto {
                file: file_name.to_owned(),
                functions,
            }],
        })
    } else {
        None
    };

    CheckOutput {
        diagnostics,
        holes,
        symbol_graph,
        typed_ast,
    }
}

fn convert_ty_diagnostic(
    data: &TyDiagnosticData,
    span: &kyokara_span::Span,
    interner: &Interner,
    item_tree: &kyokara_hir::ItemTree,
    file_name: &str,
    root: Option<&SyntaxNode>,
) -> DiagnosticDto {
    let expected_type = data
        .expected_ty()
        .map(|t| display_ty_with_tree(t, interner, item_tree));
    let actual_type = data
        .actual_ty()
        .map(|t| display_ty_with_tree(t, interner, item_tree));

    // Build the message by converting through Diagnostic.
    let diag = data.clone().into_diagnostic(*span, interner, item_tree);

    // Generate patch suggestions for fixable error codes.
    let mut fixes: Vec<FixDto> = diag
        .fixes
        .iter()
        .map(|f| convert_fix(f, file_name))
        .collect();

    let span_dto = span_dto(span, file_name);

    match data {
        TyDiagnosticData::MissingMatchArms { missing } => {
            let variant_prefix = root
                .and_then(|root| infer_match_variant_prefix(root, span.range))
                .unwrap_or_default();
            let replacement = missing
                .iter()
                .map(|v| format!("| {variant_prefix}{v} -> _"))
                .collect::<Vec<_>>()
                .join("\n");
            fixes.push(FixDto {
                message: "add missing match arms".into(),
                span: SpanDto {
                    file: span_dto.file.clone(),
                    start: span_dto.start,
                    end: span_dto.end,
                },
                replacement,
            });
        }
        TyDiagnosticData::EffectViolation { missing } => {
            let replacement = format!("with {}", missing.join(", "));
            fixes.push(FixDto {
                message: "add missing capabilities".into(),
                span: SpanDto {
                    file: span_dto.file.clone(),
                    start: span_dto.start,
                    end: span_dto.end,
                },
                replacement,
            });
        }
        _ => {}
    }

    DiagnosticDto {
        code: data.code().into(),
        severity: "error".into(),
        message: diag.message,
        span: span_dto,
        expected_type,
        actual_type,
        fixes,
    }
}

fn infer_match_variant_prefix(root: &SyntaxNode, diag_range: TextRange) -> Option<String> {
    let match_expr = root
        .descendants()
        .find_map(|n| SyntaxMatchExpr::cast(n.clone()).filter(|_| n.text_range() == diag_range))
        .or_else(|| {
            root.descendants().find_map(|n| {
                SyntaxMatchExpr::cast(n.clone()).filter(|_| {
                    let r = n.text_range();
                    r.start() <= diag_range.start() && r.end() >= diag_range.end()
                })
            })
        })?;
    let arm_list = match_expr.arm_list()?;
    for arm in arm_list.arms() {
        let Some(pat) = arm.pat() else {
            continue;
        };
        let path = match pat {
            SyntaxPat::Constructor(pat) => pat.path(),
            SyntaxPat::Ident(pat) => pat.path(),
            _ => None,
        }?;
        let segments: Vec<String> = path.segments().map(|seg| seg.text().to_string()).collect();
        if segments.len() >= 2 {
            return Some(format!("{}.", segments[..segments.len() - 1].join(".")));
        }
    }
    None
}

fn convert_lowering_diagnostic(
    diag: &kyokara_diagnostics::Diagnostic,
    code: &str,
    file_name: &str,
) -> DiagnosticDto {
    DiagnosticDto {
        code: code.into(),
        severity: severity_str(diag.severity),
        message: diag.message.clone(),
        span: span_dto(&diag.span, file_name),
        expected_type: None,
        actual_type: None,
        fixes: diag
            .fixes
            .iter()
            .map(|f| convert_fix(f, file_name))
            .collect(),
    }
}

fn convert_fix(fix: &kyokara_diagnostics::Fix, file_name: &str) -> FixDto {
    FixDto {
        message: fix.message.clone(),
        span: span_dto(&fix.span, file_name),
        replacement: fix.replacement.clone(),
    }
}

fn convert_hole(
    id: usize,
    hole: &HoleInfo,
    interner: &Interner,
    item_tree: &kyokara_hir::ItemTree,
    file_name: &str,
) -> HoleSpecDto {
    let expected_type = hole
        .expected_type
        .as_ref()
        .map(|t| display_ty_with_tree(t, interner, item_tree));

    let effects: Vec<String> = hole
        .effect_constraints
        .effects
        .iter()
        .map(|n| n.resolve(interner).to_owned())
        .collect();

    let inputs: Vec<InputVarDto> = hole
        .available_locals
        .iter()
        .map(|(name, ty)| InputVarDto {
            name: name.resolve(interner).to_owned(),
            ty: display_ty_with_tree(ty, interner, item_tree),
        })
        .collect();

    HoleSpecDto {
        id,
        name: hole.name.clone(),
        expected_type,
        effects,
        inputs,
        span: span_dto(&hole.span, file_name),
    }
}

// ── Symbol graph builder ────────────────────────────────────────────

fn symbol_id(kind: &str, name: &str, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) => format!("{kind}::{p}::{name}"),
        None => format!("{kind}::{name}"),
    }
}

fn nested_symbol_id(
    parent_kind: &str,
    parent_name: &str,
    child_name: &str,
    prefix: Option<&str>,
) -> String {
    match prefix {
        Some(p) => format!("{parent_kind}::{p}::{parent_name}::{child_name}"),
        None => format!("{parent_kind}::{parent_name}::{child_name}"),
    }
}

fn build_symbol_graph(result: &CheckResult) -> SymbolGraphDto {
    build_module_symbol_graph(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &result.interner,
        None,
    )
}

fn build_module_symbol_graph(
    item_tree: &kyokara_hir::ItemTree,
    module_scope: &ModuleScope,
    type_check: &kyokara_hir::TypeCheckResult,
    interner: &Interner,
    module_prefix: Option<&str>,
) -> SymbolGraphDto {
    // Build a lookup from function name → list of callee IDs.
    // Callee IDs use the *same* module prefix — cross-module calls get fixed up
    // later in `check_project` via a global name→ID map.
    // Build the set of known function names so we can filter out constructor
    // calls (e.g. Some, Ok, Err) that the inference engine records as callees.
    // Keep symbol-graph function nodes focused on source-defined functions.
    // Synthetic builtins (e.g. injected intrinsics) have no source range.
    let fn_names: std::collections::HashSet<&str> = item_tree
        .functions
        .iter()
        .filter(|(_, f)| f.source_range.is_some() || f.is_pub)
        .map(|(_, f)| f.name.resolve(interner))
        .collect();

    let mut call_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (caller_name, callees) in &type_check.fn_calls {
        let caller_str = caller_name.resolve(interner).to_owned();
        let callee_ids: Vec<String> = callees
            .iter()
            .filter(|n| fn_names.contains(n.resolve(interner)))
            .map(|n| symbol_id("fn", n.resolve(interner), module_prefix))
            .collect();
        call_map.insert(caller_str, callee_ids);
    }

    // Collect capability member function indices so we can exclude them
    // from the flat function list (they belong under their cap node).
    let cap_member_fns: std::collections::HashSet<kyokara_hir_def::item_tree::FnItemIdx> =
        item_tree
            .effects
            .iter()
            .flat_map(|(_, cap)| cap.functions.iter().copied())
            .collect();

    // Functions (excluding capability members).
    let mut functions: Vec<FnNodeDto> = Vec::new();
    let mut fn_id_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (idx, fn_item) in item_tree.functions.iter() {
        if fn_item.source_range.is_none() || cap_member_fns.contains(&idx) {
            continue;
        }

        let name = fn_item.name.resolve(interner).to_owned();
        let params: Vec<ParamDto> = fn_item
            .params
            .iter()
            .map(|p| ParamDto {
                name: p.name.resolve(interner).to_owned(),
                ty: kyokara_hir::display_type_ref(&p.ty, interner),
            })
            .collect();
        let return_type = fn_item
            .ret_type
            .as_ref()
            .map(|t| kyokara_hir::display_type_ref(t, interner));
        let effects: Vec<String> = fn_item
            .with_effects
            .iter()
            .filter_map(|tr| type_ref_name(tr, interner))
            .collect();
        let calls = call_map.get(&name).cloned().unwrap_or_default();

        let base_id = symbol_id("fn", &name, module_prefix);
        let count = fn_id_counts.entry(base_id.clone()).or_insert(0);
        *count += 1;
        let id = if *count == 1 {
            base_id
        } else {
            format!("{base_id}#{}", count)
        };

        functions.push(FnNodeDto {
            id,
            name,
            params,
            return_type,
            effects,
            calls,
        });
    }

    // Types.
    let mut types: Vec<TypeNodeDto> = item_tree
        .types
        .iter()
        .filter_map(|(_, type_item)| {
            let name = type_item.name.resolve(interner).to_owned();
            if name.starts_with("$core_") {
                return None;
            }
            let type_params: Vec<String> = type_item
                .type_params
                .iter()
                .map(|n| n.name.resolve(interner).to_owned())
                .collect();
            let (kind, fields, variants) = match &type_item.kind {
                TypeDefKind::Alias(TypeRef::Record {
                    fields: alias_fields,
                }) => {
                    let fs: Vec<ParamDto> = alias_fields
                        .iter()
                        .map(|(n, tr)| ParamDto {
                            name: n.resolve(interner).to_owned(),
                            ty: kyokara_hir::display_type_ref(tr, interner),
                        })
                        .collect();
                    ("record".to_owned(), fs, Vec::new())
                }
                TypeDefKind::Alias(_) => ("alias".to_owned(), Vec::new(), Vec::new()),
                TypeDefKind::Record { fields: def_fields } => {
                    let fs: Vec<ParamDto> = def_fields
                        .iter()
                        .map(|(n, tr)| ParamDto {
                            name: n.resolve(interner).to_owned(),
                            ty: kyokara_hir::display_type_ref(tr, interner),
                        })
                        .collect();
                    ("record".to_owned(), fs, Vec::new())
                }
                TypeDefKind::Adt { variants: vs } => {
                    let var_dtos: Vec<VariantDto> = vs
                        .iter()
                        .map(|v| {
                            let vname = v.name.resolve(interner).to_owned();
                            let vid = nested_symbol_id("type", &name, &vname, module_prefix);
                            VariantDto {
                                id: vid,
                                name: vname,
                                fields: v
                                    .fields
                                    .iter()
                                    .map(|tr| kyokara_hir::display_type_ref(tr, interner))
                                    .collect(),
                            }
                        })
                        .collect();
                    ("adt".to_owned(), Vec::new(), var_dtos)
                }
            };
            let id = symbol_id("type", &name, module_prefix);
            Some(TypeNodeDto {
                id,
                name,
                kind,
                type_params,
                fields,
                variants,
            })
        })
        .collect();

    let mut seen_type_names: std::collections::HashSet<String> =
        types.iter().map(|t| t.name.clone()).collect();
    for (visible_name, (module_name, _)) in &module_scope.imported_type_namespaces {
        if module_name.resolve(interner) != "collections" {
            continue;
        }
        let Some(&type_idx) = module_scope.types.get(visible_name) else {
            continue;
        };
        let Some(core) = module_scope.core_types.kind_for_idx(type_idx) else {
            continue;
        };
        if !is_collection_core_type(core) {
            continue;
        }
        let type_item = &item_tree.types[type_idx];
        let visible_name_str = visible_name.resolve(interner).to_owned();
        if !seen_type_names.insert(visible_name_str.clone()) {
            continue;
        }
        types.push(type_node_from_item(
            type_item,
            &visible_name_str,
            interner,
            module_prefix,
        ));
    }

    // Capabilities — also emit member functions as fn nodes with cap-qualified IDs.
    let mut cap_member_fn_nodes: Vec<FnNodeDto> = Vec::new();
    let capabilities: Vec<CapNodeDto> = item_tree
        .effects
        .iter()
        .map(|(_, cap_item)| {
            let name = cap_item.name.resolve(interner).to_owned();
            let fns: Vec<String> = cap_item
                .functions
                .iter()
                .map(|&fn_idx| {
                    let fn_item = &item_tree.functions[fn_idx];
                    let fn_name = fn_item.name.resolve(interner).to_owned();
                    let member_id = nested_symbol_id("cap", &name, &fn_name, module_prefix);
                    let params: Vec<ParamDto> = fn_item
                        .params
                        .iter()
                        .map(|p| ParamDto {
                            name: p.name.resolve(interner).to_owned(),
                            ty: kyokara_hir::display_type_ref(&p.ty, interner),
                        })
                        .collect();
                    let return_type = fn_item
                        .ret_type
                        .as_ref()
                        .map(|t| kyokara_hir::display_type_ref(t, interner));
                    cap_member_fn_nodes.push(FnNodeDto {
                        id: member_id.clone(),
                        name: fn_name,
                        params,
                        return_type,
                        effects: Vec::new(),
                        calls: Vec::new(),
                    });
                    member_id
                })
                .collect();
            let id = symbol_id("cap", &name, module_prefix);
            CapNodeDto {
                id,
                name,
                functions: fns,
            }
        })
        .collect();

    // Merge cap member fn nodes into the function list.
    functions.extend(cap_member_fn_nodes);

    SymbolGraphDto {
        partial: false,
        functions,
        types,
        capabilities,
    }
}

fn is_collection_core_type(core: CoreType) -> bool {
    matches!(
        core,
        CoreType::List
            | CoreType::BitSet
            | CoreType::Map
            | CoreType::Set
            | CoreType::Deque
            | CoreType::MutableList
            | CoreType::MutableDeque
            | CoreType::MutablePriorityQueue
            | CoreType::MutableMap
            | CoreType::MutableSet
            | CoreType::MutableBitSet
    )
}

fn type_node_from_item(
    type_item: &kyokara_hir_def::item_tree::TypeItem,
    visible_name: &str,
    interner: &Interner,
    module_prefix: Option<&str>,
) -> TypeNodeDto {
    let type_params: Vec<String> = type_item
        .type_params
        .iter()
        .map(|n| n.name.resolve(interner).to_owned())
        .collect();
    let (kind, fields, variants) = match &type_item.kind {
        TypeDefKind::Alias(TypeRef::Record {
            fields: alias_fields,
        }) => {
            let fs: Vec<ParamDto> = alias_fields
                .iter()
                .map(|(n, tr)| ParamDto {
                    name: n.resolve(interner).to_owned(),
                    ty: kyokara_hir::display_type_ref(tr, interner),
                })
                .collect();
            ("record".to_owned(), fs, Vec::new())
        }
        TypeDefKind::Alias(_) => ("alias".to_owned(), Vec::new(), Vec::new()),
        TypeDefKind::Record { fields: def_fields } => {
            let fs: Vec<ParamDto> = def_fields
                .iter()
                .map(|(n, tr)| ParamDto {
                    name: n.resolve(interner).to_owned(),
                    ty: kyokara_hir::display_type_ref(tr, interner),
                })
                .collect();
            ("record".to_owned(), fs, Vec::new())
        }
        TypeDefKind::Adt { variants: vs } => {
            let var_dtos: Vec<VariantDto> = vs
                .iter()
                .map(|v| {
                    let vname = v.name.resolve(interner).to_owned();
                    let vid = nested_symbol_id("type", visible_name, &vname, module_prefix);
                    VariantDto {
                        id: vid,
                        name: vname,
                        fields: v
                            .fields
                            .iter()
                            .map(|tr| kyokara_hir::display_type_ref(tr, interner))
                            .collect(),
                    }
                })
                .collect();
            ("adt".to_owned(), Vec::new(), var_dtos)
        }
    };
    let id = symbol_id("type", visible_name, module_prefix);
    TypeNodeDto {
        id,
        name: visible_name.to_owned(),
        kind,
        type_params,
        fields,
        variants,
    }
}

fn dedupe_call_ids(calls: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    calls.retain(|call| seen.insert(call.clone()));
}

/// Render a surface-level TypeRef as a human-readable string (for the symbol graph).
/// Extract the leaf name from a TypeRef::Path (used for effect/capability names).
fn type_ref_name(tr: &TypeRef, interner: &Interner) -> Option<String> {
    if let TypeRef::Path { path, .. } = tr {
        path.last().map(|n| n.resolve(interner).to_owned())
    } else {
        None
    }
}

fn span_dto(span: &kyokara_span::Span, file_name: &str) -> SpanDto {
    SpanDto {
        file: file_name.into(),
        start: span.range.start().into(),
        end: span.range.end().into(),
    }
}

fn severity_str(sev: Severity) -> String {
    match sev {
        Severity::Error => "error".into(),
        Severity::Warning => "warning".into(),
        Severity::Info => "info".into(),
        Severity::Hint => "hint".into(),
    }
}

fn expr_idx_to_u32(idx: ExprIdx) -> u32 {
    idx.into_raw().into_u32()
}

fn pat_idx_to_u32(idx: PatIdx) -> u32 {
    idx.into_raw().into_u32()
}

fn local_symbol_id(fn_id: &str, pat_idx: PatIdx) -> String {
    format!("local::{fn_id}::{}", pat_idx_to_u32(pat_idx))
}

fn range_to_span(range: TextRange, file_name: &str) -> SpanDto {
    SpanDto {
        file: file_name.to_owned(),
        start: range.start().into(),
        end: range.end().into(),
    }
}

fn build_function_id_map(
    item_tree: &ItemTree,
    interner: &Interner,
    module_prefix: Option<&str>,
) -> FxHashMap<FnItemIdx, String> {
    let cap_member_fns: std::collections::HashSet<FnItemIdx> = item_tree
        .effects
        .iter()
        .flat_map(|(_, cap)| cap.functions.iter().copied())
        .collect();

    let mut fn_id_map = FxHashMap::default();
    let mut fn_id_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for (idx, fn_item) in item_tree.functions.iter() {
        if fn_item.source_range.is_none() || cap_member_fns.contains(&idx) {
            continue;
        }
        let name = fn_item.name.resolve(interner);
        let base_id = symbol_id("fn", name, module_prefix);
        let count = fn_id_counts.entry(base_id.clone()).or_insert(0);
        *count += 1;
        let id = if *count == 1 {
            base_id
        } else {
            format!("{base_id}#{}", count)
        };
        fn_id_map.insert(idx, id);
    }

    fn_id_map
}

fn build_typed_ast_for_module(
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    type_check: &TypeCheckResult,
    interner: &Interner,
    file_name: &str,
    module_prefix: Option<&str>,
) -> (Vec<TypedFnAstDto>, bool) {
    let fn_id_map = build_function_id_map(item_tree, interner, module_prefix);
    let mut functions = Vec::new();
    let mut partial = false;

    for (fn_idx, fn_item) in item_tree.functions.iter() {
        let Some(fn_id) = fn_id_map.get(&fn_idx) else {
            continue;
        };
        if !fn_item.has_body || fn_item.source_range.is_none() {
            continue;
        }

        let Some(body) = type_check.fn_bodies.get(&fn_idx) else {
            partial = true;
            continue;
        };
        let Some(result) = type_check.fn_results.get(&fn_idx) else {
            partial = true;
            continue;
        };

        functions.push(build_typed_fn_ast(
            fn_id,
            fn_item.name.resolve(interner),
            fn_item.source_range,
            body,
            result,
            module_scope,
            item_tree,
            interner,
            file_name,
            &fn_id_map,
            module_prefix,
        ));
    }

    (functions, partial)
}

#[allow(clippy::too_many_arguments)]
fn build_typed_fn_ast(
    fn_id: &str,
    fn_name: &str,
    fn_range: Option<TextRange>,
    body: &kyokara_hir::Body,
    inference: &kyokara_hir::InferenceResult,
    module_scope: &ModuleScope,
    item_tree: &ItemTree,
    interner: &Interner,
    file_name: &str,
    fn_id_map: &FxHashMap<FnItemIdx, String>,
    module_prefix: Option<&str>,
) -> TypedFnAstDto {
    let mut expr_nodes = Vec::new();
    for (expr_idx, expr) in body.exprs.iter() {
        let id = expr_idx_to_u32(expr_idx);
        let span = body
            .expr_source_map
            .get(expr_idx)
            .copied()
            .or(fn_range)
            .map(|range| range_to_span(range, file_name))
            .unwrap_or_else(|| range_to_span(TextRange::default(), file_name));

        let ty = inference
            .expr_types
            .get(expr_idx)
            .map(|t| display_ty_with_tree(t, interner, item_tree))
            .unwrap_or_else(|| "<unknown>".to_owned());

        expr_nodes.push(TypedExprNodeDto {
            id,
            kind: expr_kind(expr).to_owned(),
            span,
            ty,
            expr_refs: expr_expr_refs(expr),
            pat_refs: expr_pat_refs(expr),
            symbol: expr_symbol(
                expr_idx,
                expr,
                body,
                module_scope,
                item_tree,
                interner,
                fn_id,
                fn_id_map,
                module_prefix,
            ),
        });
    }
    expr_nodes.sort_by_key(|node| node.id);

    let mut pat_nodes = Vec::new();
    for (pat_idx, pat) in body.pats.iter() {
        let id = pat_idx_to_u32(pat_idx);
        let span = body
            .pat_source_map
            .get(pat_idx)
            .copied()
            .or(fn_range)
            .map(|range| range_to_span(range, file_name))
            .unwrap_or_else(|| range_to_span(TextRange::default(), file_name));

        let ty = inference
            .pat_types
            .get(pat_idx)
            .map(|t| display_ty_with_tree(t, interner, item_tree))
            .unwrap_or_else(|| "<unknown>".to_owned());

        pat_nodes.push(TypedPatNodeDto {
            id,
            kind: pat_kind(pat).to_owned(),
            span,
            ty,
            pat_refs: pat_refs(pat),
            symbol: pat_symbol(fn_id, pat_idx, pat),
        });
    }
    pat_nodes.sort_by_key(|node| node.id);

    TypedFnAstDto {
        id: fn_id.to_owned(),
        name: fn_name.to_owned(),
        root_expr: expr_idx_to_u32(body.root),
        expr_nodes,
        pat_nodes,
    }
}

fn expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Literal(_) => "Literal",
        Expr::Path(_) => "Path",
        Expr::Binary { .. } => "Binary",
        Expr::Unary { .. } => "Unary",
        Expr::Call { .. } => "Call",
        Expr::Field { .. } => "Field",
        Expr::Index { .. } => "Index",
        Expr::If { .. } => "If",
        Expr::Match { .. } => "Match",
        Expr::Block { .. } => "Block",
        Expr::Return(_) => "Return",
        Expr::RecordLit { .. } => "RecordLit",
        Expr::Lambda { .. } => "Lambda",
        Expr::Old(_) => "Old",
        Expr::Hole => "Hole",
        Expr::Missing => "Missing",
    }
}

fn pat_kind(pat: &Pat) -> &'static str {
    match pat {
        Pat::Bind { .. } => "Bind",
        Pat::Wildcard => "Wildcard",
        Pat::Literal(_) => "Literal",
        Pat::Constructor { .. } => "Constructor",
        Pat::Record { .. } => "Record",
        Pat::Missing => "Missing",
    }
}

fn expr_expr_refs(expr: &Expr) -> Vec<u32> {
    let mut refs = Vec::new();
    match expr {
        Expr::Binary { lhs, rhs, .. } => {
            refs.push(expr_idx_to_u32(*lhs));
            refs.push(expr_idx_to_u32(*rhs));
        }
        Expr::Unary { operand, .. } => refs.push(expr_idx_to_u32(*operand)),
        Expr::Call { callee, args } => {
            refs.push(expr_idx_to_u32(*callee));
            for arg in args {
                match arg {
                    CallArg::Positional(expr) => refs.push(expr_idx_to_u32(*expr)),
                    CallArg::Named { value, .. } => refs.push(expr_idx_to_u32(*value)),
                }
            }
        }
        Expr::Field { base, .. } => refs.push(expr_idx_to_u32(*base)),
        Expr::Index { base, index } => {
            refs.push(expr_idx_to_u32(*base));
            refs.push(expr_idx_to_u32(*index));
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            refs.push(expr_idx_to_u32(*condition));
            refs.push(expr_idx_to_u32(*then_branch));
            if let Some(else_expr) = else_branch {
                refs.push(expr_idx_to_u32(*else_expr));
            }
        }
        Expr::Match { scrutinee, arms } => {
            refs.push(expr_idx_to_u32(*scrutinee));
            for MatchArm { body, .. } in arms {
                refs.push(expr_idx_to_u32(*body));
            }
        }
        Expr::Block { stmts, tail } => {
            for stmt in stmts {
                match stmt {
                    Stmt::Let { init, .. } => refs.push(expr_idx_to_u32(*init)),
                    Stmt::Assign { target, value } => {
                        refs.push(expr_idx_to_u32(*target));
                        refs.push(expr_idx_to_u32(*value));
                    }
                    Stmt::While { condition, body } => {
                        refs.push(expr_idx_to_u32(*condition));
                        refs.push(expr_idx_to_u32(*body));
                    }
                    Stmt::For { source, body, .. } => {
                        refs.push(expr_idx_to_u32(*source));
                        refs.push(expr_idx_to_u32(*body));
                    }
                    Stmt::Expr(expr) => refs.push(expr_idx_to_u32(*expr)),
                    Stmt::Break | Stmt::Continue => {}
                }
            }
            if let Some(expr) = tail {
                refs.push(expr_idx_to_u32(*expr));
            }
        }
        Expr::Return(value) => {
            if let Some(expr) = value {
                refs.push(expr_idx_to_u32(*expr));
            }
        }
        Expr::RecordLit { fields, .. } => {
            for (_, value) in fields {
                refs.push(expr_idx_to_u32(*value));
            }
        }
        Expr::Lambda { body, .. } => refs.push(expr_idx_to_u32(*body)),
        Expr::Old(inner) => refs.push(expr_idx_to_u32(*inner)),
        Expr::Literal(_) | Expr::Path(_) | Expr::Hole | Expr::Missing => {}
    }
    refs
}

fn expr_pat_refs(expr: &Expr) -> Vec<u32> {
    let mut refs = Vec::new();
    match expr {
        Expr::Match { arms, .. } => {
            for MatchArm { pat, .. } in arms {
                refs.push(pat_idx_to_u32(*pat));
            }
        }
        Expr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    Stmt::Let { pat, .. } | Stmt::For { pat, .. } => {
                        refs.push(pat_idx_to_u32(*pat))
                    }
                    Stmt::Assign { .. }
                    | Stmt::While { .. }
                    | Stmt::Break
                    | Stmt::Continue
                    | Stmt::Expr(_) => {}
                }
            }
        }
        Expr::Lambda { params, .. } => {
            for (pat, _) in params {
                refs.push(pat_idx_to_u32(*pat));
            }
        }
        _ => {}
    }
    refs
}

fn pat_refs(pat: &Pat) -> Vec<u32> {
    match pat {
        Pat::Constructor { args, .. } => args.iter().map(|p| pat_idx_to_u32(*p)).collect(),
        Pat::Bind { .. } | Pat::Wildcard | Pat::Literal(_) | Pat::Record { .. } | Pat::Missing => {
            Vec::new()
        }
    }
}

fn pat_symbol(fn_id: &str, pat_idx: PatIdx, pat: &Pat) -> Option<String> {
    match pat {
        Pat::Bind { .. } => Some(local_symbol_id(fn_id, pat_idx)),
        Pat::Wildcard
        | Pat::Literal(_)
        | Pat::Constructor { .. }
        | Pat::Record { .. }
        | Pat::Missing => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn expr_symbol(
    expr_idx: ExprIdx,
    expr: &Expr,
    body: &kyokara_hir::Body,
    module_scope: &ModuleScope,
    item_tree: &ItemTree,
    interner: &Interner,
    fn_id: &str,
    fn_id_map: &FxHashMap<FnItemIdx, String>,
    module_prefix: Option<&str>,
) -> Option<String> {
    let Expr::Path(path) = expr else {
        return None;
    };
    if !path.is_single() {
        return None;
    }
    let name = path.segments[0];
    let resolved = body.resolve_name_at(module_scope, expr_idx, name)?;
    if let Some((pat_idx, _)) = resolved.local_binding {
        return Some(local_symbol_id(fn_id, pat_idx));
    }
    resolved_symbol_id(
        &resolved.resolved,
        item_tree,
        fn_id,
        fn_id_map,
        interner,
        module_prefix,
    )
}

fn resolved_symbol_id(
    resolved: &ResolvedName,
    item_tree: &ItemTree,
    fn_id: &str,
    fn_id_map: &FxHashMap<FnItemIdx, String>,
    interner: &Interner,
    module_prefix: Option<&str>,
) -> Option<String> {
    match resolved {
        ResolvedName::Local(ScopeDef::Local(pat_idx)) => Some(local_symbol_id(fn_id, *pat_idx)),
        ResolvedName::Fn(fn_idx) => fn_id_map.get(fn_idx).cloned(),
        ResolvedName::FnFamily(_) => None,
        ResolvedName::Type(type_idx) => {
            let name = item_tree.types[*type_idx].name.resolve(interner);
            Some(symbol_id("type", name, module_prefix))
        }
        ResolvedName::Effect(effect_idx) => {
            let name = item_tree.effects[*effect_idx].name.resolve(interner);
            Some(symbol_id("cap", name, module_prefix))
        }
        ResolvedName::Constructor {
            type_idx,
            variant_idx,
        } => {
            let type_item = &item_tree.types[*type_idx];
            let TypeDefKind::Adt { variants } = &type_item.kind else {
                return None;
            };
            let variant = variants.get(*variant_idx)?;
            Some(nested_symbol_id(
                "type",
                type_item.name.resolve(interner),
                variant.name.resolve(interner),
                module_prefix,
            ))
        }
        ResolvedName::Let(_)
        | ResolvedName::Import(_)
        | ResolvedName::Trait(_)
        | ResolvedName::Module(_)
        | ResolvedName::StaticMethodType(_)
        | ResolvedName::Local(_) => None,
    }
}

// ── Refactor DTOs ───────────────────────────────────────────────────

/// Output of a refactor operation.
#[derive(Debug, Serialize)]
pub struct RefactorOutputDto {
    pub description: String,
    pub status: String,
    pub verified: bool,
    pub edits: Vec<TextEditDto>,
    pub error: Option<String>,
    pub verification_diagnostics: Vec<VerificationDiagnosticDto>,
    pub patched_sources: Option<Vec<PatchedSourceDto>>,
}

/// A structured verification diagnostic from post-refactor type checking.
#[derive(Debug, Serialize)]
pub struct VerificationDiagnosticDto {
    pub message: String,
    pub code: Option<String>,
    pub span: Option<SpanDto>,
}

/// A patched source file from a transactional refactor.
#[derive(Debug, Serialize)]
pub struct PatchedSourceDto {
    pub file: String,
    pub source: String,
}

/// A single text edit in a refactor result.
#[derive(Debug, Serialize)]
pub struct TextEditDto {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

// ── Refactor entry points ───────────────────────────────────────────

/// Run a refactor on a single source file and return structured output.
///
/// When `force` is true, verification is skipped and status is "skipped".
pub fn refactor(
    source: &str,
    file_name: &str,
    action: kyokara_refactor::RefactorAction,
    force: bool,
) -> RefactorOutputDto {
    let result = kyokara_hir::check_file(source);
    let file_id = kyokara_span::FileId(0);

    let tx = if force {
        kyokara_refactor::transaction::transact_force(source, &result, file_id, action)
    } else {
        kyokara_refactor::transaction::transact(source, &result, file_id, action)
    };

    match tx {
        Ok(t) => transaction_to_dto(t, |fid| {
            if fid == file_id {
                file_name.to_string()
            } else {
                format!("<unresolved:FileId({})>", fid.0)
            }
        }),
        Err(e) => error_dto(e),
    }
}

/// Run a refactor on a multi-file project and return structured output.
///
/// When `force` is true, verification is skipped and status is "skipped".
pub fn refactor_project(
    entry_file: &std::path::Path,
    action: kyokara_refactor::RefactorAction,
    force: bool,
) -> RefactorOutputDto {
    let result = kyokara_hir::check_project(entry_file);

    let tx = if force {
        kyokara_refactor::transaction::transact_project_force(entry_file, &result, action)
    } else {
        kyokara_refactor::transaction::transact_project(entry_file, &result, action)
    };

    let file_map = result.file_map;
    match tx {
        Ok(t) => transaction_to_dto(t, |fid| {
            file_map
                .path(fid)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| format!("<unresolved:FileId({})>", fid.0))
        }),
        Err(e) => error_dto(e),
    }
}

fn transaction_to_dto(
    t: kyokara_refactor::TransactionResult,
    file_name: impl Fn(kyokara_span::FileId) -> String,
) -> RefactorOutputDto {
    let edits = t
        .refactor
        .edits
        .iter()
        .map(|e| TextEditDto {
            file: file_name(e.file_id),
            start: e.range.start().into(),
            end: e.range.end().into(),
            new_text: e.new_text.clone(),
        })
        .collect();

    let patched_sources: Vec<PatchedSourceDto> = t
        .patched_sources
        .iter()
        .map(|(fid, src)| PatchedSourceDto {
            file: file_name(*fid),
            source: src.clone(),
        })
        .collect();

    let (status, verified, verification_diagnostics) = match &t.verification {
        kyokara_refactor::VerificationStatus::Verified => {
            ("typechecked".to_string(), true, Vec::new())
        }
        kyokara_refactor::VerificationStatus::Failed { diagnostics } => {
            let dtos: Vec<VerificationDiagnosticDto> = diagnostics
                .iter()
                .map(|d| VerificationDiagnosticDto {
                    message: d.message.clone(),
                    code: d.code.clone(),
                    span: d.span.map(|s| span_dto(&s, &file_name(s.file))),
                })
                .collect();
            ("failed".to_string(), false, dtos)
        }
        kyokara_refactor::VerificationStatus::Skipped => ("skipped".to_string(), false, Vec::new()),
    };

    RefactorOutputDto {
        description: t.refactor.description,
        status,
        verified,
        edits,
        error: None,
        verification_diagnostics,
        patched_sources: Some(patched_sources),
    }
}

fn error_dto(e: kyokara_refactor::RefactorError) -> RefactorOutputDto {
    RefactorOutputDto {
        description: String::new(),
        status: "error".into(),
        verified: false,
        edits: Vec::new(),
        error: Some(e.to_string()),
        verification_diagnostics: Vec::new(),
        patched_sources: None,
    }
}
