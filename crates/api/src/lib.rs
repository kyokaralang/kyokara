//! `kyokara-api` — Compiler-as-API with JSON serialization.
//!
//! This crate owns all serialization. Internal compiler types stay
//! `serde`-free; instead, `api` defines its own DTO types that mirror
//! the internal structures and derive `Serialize`.
//!
//! Outputs (v0.0):
//! - `CheckOutput` with structured diagnostics, typed hole specs, and symbol graph

use kyokara_diagnostics::Severity;
use kyokara_hir::{
    CheckResult, HoleInfo, TyDiagnosticData, TypeDefKind, TypeRef, display_ty_with_tree,
};
use kyokara_intern::Interner;
use serde::Serialize;

// ── Top-level output ────────────────────────────────────────────────

/// Top-level output from the check pipeline.
#[derive(Debug, Serialize)]
pub struct CheckOutput {
    pub diagnostics: Vec<DiagnosticDto>,
    pub holes: Vec<HoleSpecDto>,
    pub symbol_graph: SymbolGraphDto,
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

// ── Public entry point ──────────────────────────────────────────────

/// Run the full check pipeline on source text and return structured output.
pub fn check(source: &str, file_name: &str) -> CheckOutput {
    let result = kyokara_hir::check_file(source);
    convert_result(&result, file_name)
}

/// Run the check pipeline on a multi-file project and return structured output.
pub fn check_project(entry_file: &std::path::Path) -> CheckOutput {
    let result = kyokara_hir::check_project(entry_file);
    let mut diagnostics = Vec::new();
    let interner = &result.interner;

    // Aggregate parse errors from all modules.
    for (mod_path, errors) in &result.parse_errors {
        let file_name = result
            .file_map
            .path(
                result
                    .module_graph
                    .get(mod_path)
                    .map(|i| i.file_id)
                    .unwrap_or(kyokara_span::FileId(0)),
            )
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".into());

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
        let code = if diag.message.contains("duplicate") {
            "E0102"
        } else {
            "E0101"
        };
        let file_name = result
            .file_map
            .path(diag.span.file)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<project>".into());
        diagnostics.push(convert_lowering_diagnostic(diag, code, &file_name));
    }

    // Type-check diagnostics from all modules.
    for (mod_path, tc) in &result.type_checks {
        let file_name = result
            .module_graph
            .get(mod_path)
            .and_then(|i| result.file_map.path(i.file_id))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".into());

        let item_tree = result.module_graph.get(mod_path).map(|i| &i.item_tree);

        // Body lowering diagnostics (unresolved names, duplicates).
        for diag in &tc.body_lowering_diagnostics {
            let code = if diag.message.contains("duplicate") {
                "E0102"
            } else {
                "E0101"
            };
            diagnostics.push(convert_lowering_diagnostic(diag, code, &file_name));
        }

        for (data, span) in &tc.raw_diagnostics {
            if let Some(tree) = item_tree {
                diagnostics.push(convert_ty_diagnostic(
                    data, span, interner, tree, &file_name,
                ));
            }
        }
    }

    // Collect holes and symbol graphs from all modules.
    let mut holes = Vec::new();
    let mut all_functions = Vec::new();
    let mut all_types = Vec::new();
    let mut all_capabilities = Vec::new();

    let builtin_names: std::collections::HashSet<&str> =
        ["Option", "Result", "List", "Map"].into_iter().collect();
    let mut seen_builtins: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (mod_path, tc) in &result.type_checks {
        let file_name = result
            .module_graph
            .get(mod_path)
            .and_then(|i| result.file_map.path(i.file_id))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".into());

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
            let graph = build_module_symbol_graph(&info.item_tree, tc, interner, prefix.as_deref());
            all_functions.extend(graph.functions);
            all_capabilities.extend(graph.capabilities);

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
        }
    }

    // Post-process call edges: build a global fn_name → qualified_id map,
    // then rewrite any call edge that doesn't match an emitted function ID.
    let fn_name_to_id: std::collections::HashMap<String, String> = all_functions
        .iter()
        .map(|f| (f.name.clone(), f.id.clone()))
        .collect();
    for func in &mut all_functions {
        for call in &mut func.calls {
            // If the call ID doesn't match any emitted function, look up by name.
            if !fn_name_to_id.values().any(|id| id == call) {
                // Extract the callee name (last segment after "fn::").
                let callee_name = call.strip_prefix("fn::").unwrap_or(call);
                // Strip any module prefix to get the bare name.
                let bare_name = callee_name.rsplit("::").next().unwrap_or(callee_name);
                if let Some(qualified) = fn_name_to_id.get(bare_name) {
                    *call = qualified.clone();
                }
            }
        }
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
    }
}

// ── Conversion helpers ──────────────────────────────────────────────

fn convert_result(result: &CheckResult, file_name: &str) -> CheckOutput {
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
        let code = if diag.message.contains("duplicate") {
            "E0102"
        } else {
            "E0101"
        };
        diagnostics.push(convert_lowering_diagnostic(diag, code, file_name));
    }

    // Body lowering diagnostics (unresolved names, duplicates from body lowering).
    for diag in &result.type_check.body_lowering_diagnostics {
        let code = if diag.message.contains("duplicate") {
            "E0102"
        } else {
            "E0101"
        };
        diagnostics.push(convert_lowering_diagnostic(diag, code, file_name));
    }

    // Type-checker raw diagnostics.
    for (data, span) in &result.type_check.raw_diagnostics {
        diagnostics.push(convert_ty_diagnostic(
            data,
            span,
            interner,
            &result.item_tree,
            file_name,
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
    symbol_graph.partial = !result.parse_errors.is_empty();

    CheckOutput {
        diagnostics,
        holes,
        symbol_graph,
    }
}

fn convert_ty_diagnostic(
    data: &TyDiagnosticData,
    span: &kyokara_span::Span,
    interner: &Interner,
    item_tree: &kyokara_hir::ItemTree,
    file_name: &str,
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
            let replacement = missing
                .iter()
                .map(|v| format!("| {v} -> _"))
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
        .caps
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
        &result.type_check,
        &result.interner,
        None,
    )
}

fn build_module_symbol_graph(
    item_tree: &kyokara_hir::ItemTree,
    type_check: &kyokara_hir::TypeCheckResult,
    interner: &Interner,
    module_prefix: Option<&str>,
) -> SymbolGraphDto {
    // Build a lookup from function name → list of callee IDs.
    // Callee IDs use the *same* module prefix — cross-module calls get fixed up
    // later in `check_project` via a global name→ID map.
    // Build the set of known function names so we can filter out constructor
    // calls (e.g. Some, Ok, Err) that the inference engine records as callees.
    let fn_names: std::collections::HashSet<&str> = item_tree
        .functions
        .iter()
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
            .caps
            .iter()
            .flat_map(|(_, cap)| cap.functions.iter().copied())
            .collect();

    // Functions (excluding capability members).
    let functions: Vec<FnNodeDto> = item_tree
        .functions
        .iter()
        .filter(|(idx, _)| !cap_member_fns.contains(idx))
        .map(|(_, fn_item)| {
            let name = fn_item.name.resolve(interner).to_owned();
            let params: Vec<ParamDto> = fn_item
                .params
                .iter()
                .map(|p| ParamDto {
                    name: p.name.resolve(interner).to_owned(),
                    ty: display_type_ref(&p.ty, interner),
                })
                .collect();
            let return_type = fn_item
                .ret_type
                .as_ref()
                .map(|t| display_type_ref(t, interner));
            let effects: Vec<String> = fn_item
                .with_caps
                .iter()
                .filter_map(|tr| type_ref_name(tr, interner))
                .collect();
            let calls = call_map.get(&name).cloned().unwrap_or_default();
            let id = symbol_id("fn", &name, module_prefix);
            FnNodeDto {
                id,
                name,
                params,
                return_type,
                effects,
                calls,
            }
        })
        .collect();

    // Types.
    let types: Vec<TypeNodeDto> = item_tree
        .types
        .iter()
        .map(|(_, type_item)| {
            let name = type_item.name.resolve(interner).to_owned();
            let type_params: Vec<String> = type_item
                .type_params
                .iter()
                .map(|n| n.resolve(interner).to_owned())
                .collect();
            let (kind, fields, variants) = match &type_item.kind {
                TypeDefKind::Alias(_) => ("alias".to_owned(), Vec::new(), Vec::new()),
                TypeDefKind::Record { fields: def_fields } => {
                    let fs: Vec<ParamDto> = def_fields
                        .iter()
                        .map(|(n, tr)| ParamDto {
                            name: n.resolve(interner).to_owned(),
                            ty: display_type_ref(tr, interner),
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
                                    .map(|tr| display_type_ref(tr, interner))
                                    .collect(),
                            }
                        })
                        .collect();
                    ("adt".to_owned(), Vec::new(), var_dtos)
                }
            };
            let id = symbol_id("type", &name, module_prefix);
            TypeNodeDto {
                id,
                name,
                kind,
                type_params,
                fields,
                variants,
            }
        })
        .collect();

    // Capabilities — also emit member functions as fn nodes with cap-qualified IDs.
    let mut cap_member_fn_nodes: Vec<FnNodeDto> = Vec::new();
    let capabilities: Vec<CapNodeDto> = item_tree
        .caps
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
                            ty: display_type_ref(&p.ty, interner),
                        })
                        .collect();
                    let return_type = fn_item
                        .ret_type
                        .as_ref()
                        .map(|t| display_type_ref(t, interner));
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
    let mut functions = functions;
    functions.extend(cap_member_fn_nodes);

    SymbolGraphDto {
        partial: false,
        functions,
        types,
        capabilities,
    }
}

/// Render a surface-level TypeRef as a human-readable string (for the symbol graph).
fn display_type_ref(tr: &TypeRef, interner: &Interner) -> String {
    match tr {
        TypeRef::Path { path, args } => {
            let base: String = path
                .segments
                .iter()
                .map(|s| s.resolve(interner))
                .collect::<Vec<_>>()
                .join(".");
            if args.is_empty() {
                base
            } else {
                let arg_strs: Vec<String> =
                    args.iter().map(|a| display_type_ref(a, interner)).collect();
                format!("{base}<{}>", arg_strs.join(", "))
            }
        }
        TypeRef::Fn { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|p| display_type_ref(p, interner))
                .collect();
            format!(
                "fn({}) -> {}",
                ps.join(", "),
                display_type_ref(ret, interner)
            )
        }
        TypeRef::Record { fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{}: {}", n.resolve(interner), display_type_ref(t, interner)))
                .collect();
            format!("{{ {} }}", fs.join(", "))
        }
        TypeRef::Refined { name, base, .. } => {
            format!(
                "{{ {}: {} | ... }}",
                name.resolve(interner),
                display_type_ref(base, interner)
            )
        }
        TypeRef::Error => "<error>".into(),
    }
}

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
                "<unknown>".into()
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
                .unwrap_or_else(|| "<unknown>".into())
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
