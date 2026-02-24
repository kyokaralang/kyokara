//! `kyokara-api` — Compiler-as-API with JSON serialization.
//!
//! This crate owns all serialization. Internal compiler types stay
//! `serde`-free; instead, `api` defines its own DTO types that mirror
//! the internal structures and derive `Serialize`.
//!
//! Outputs (v0.0):
//! - `CheckOutput` with structured diagnostics, typed hole specs, and symbol graph

use kyokara_diagnostics::Severity;
use kyokara_hir::{CheckResult, HoleInfo, TyDiagnosticData, TypeDefKind, TypeRef, display_ty};
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
}

/// A function node in the symbol graph.
#[derive(Debug, Serialize)]
pub struct FnNodeDto {
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
    pub name: String,
    pub kind: String,
    pub type_params: Vec<String>,
    pub fields: Vec<ParamDto>,
    pub variants: Vec<VariantDto>,
}

/// A variant of an ADT in the symbol graph.
#[derive(Debug, Serialize)]
pub struct VariantDto {
    pub name: String,
    pub fields: Vec<String>,
}

/// A capability node in the symbol graph.
#[derive(Debug, Serialize)]
pub struct CapNodeDto {
    pub name: String,
    pub functions: Vec<String>,
}

// ── Public entry point ──────────────────────────────────────────────

/// Run the full check pipeline on source text and return structured output.
pub fn check(source: &str, file_name: &str) -> CheckOutput {
    let result = kyokara_hir::check_file(source);
    convert_result(&result, file_name)
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
                start: 0,
                end: 0,
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

    // Type-checker raw diagnostics.
    for (data, span) in &result.type_check.raw_diagnostics {
        diagnostics.push(convert_ty_diagnostic(data, span, interner, file_name));
    }

    // Collect hole specs from all function results.
    let mut holes = Vec::new();
    for fn_result in result.type_check.fn_results.values() {
        for hole in &fn_result.holes {
            holes.push(convert_hole(holes.len(), hole, interner, file_name));
        }
    }

    // Build symbol graph.
    let symbol_graph = build_symbol_graph(result);

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
    file_name: &str,
) -> DiagnosticDto {
    let expected_type = data.expected_ty().map(|t| display_ty(t, interner));
    let actual_type = data.actual_ty().map(|t| display_ty(t, interner));

    // Build the message by converting through Diagnostic.
    let diag = data.clone().into_diagnostic(*span, interner);

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

fn convert_hole(id: usize, hole: &HoleInfo, interner: &Interner, file_name: &str) -> HoleSpecDto {
    let expected_type = hole.expected_type.as_ref().map(|t| display_ty(t, interner));

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
            ty: display_ty(ty, interner),
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

fn build_symbol_graph(result: &CheckResult) -> SymbolGraphDto {
    let interner = &result.interner;
    let item_tree = &result.item_tree;

    // Build a lookup from function name → list of callee names.
    let mut call_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (caller_name, callees) in &result.type_check.fn_calls {
        let caller_str = caller_name.resolve(interner).to_owned();
        let callee_strs: Vec<String> = callees
            .iter()
            .map(|n| n.resolve(interner).to_owned())
            .collect();
        call_map.insert(caller_str, callee_strs);
    }

    // Functions.
    let functions: Vec<FnNodeDto> = item_tree
        .functions
        .iter()
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
            FnNodeDto {
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
                        .map(|v| VariantDto {
                            name: v.name.resolve(interner).to_owned(),
                            fields: v
                                .fields
                                .iter()
                                .map(|tr| display_type_ref(tr, interner))
                                .collect(),
                        })
                        .collect();
                    ("adt".to_owned(), Vec::new(), var_dtos)
                }
            };
            TypeNodeDto {
                name,
                kind,
                type_params,
                fields,
                variants,
            }
        })
        .collect();

    // Capabilities.
    let capabilities: Vec<CapNodeDto> = item_tree
        .caps
        .iter()
        .map(|(_, cap_item)| {
            let name = cap_item.name.resolve(interner).to_owned();
            let fns: Vec<String> = cap_item
                .functions
                .iter()
                .map(|&fn_idx| {
                    item_tree.functions[fn_idx]
                        .name
                        .resolve(interner)
                        .to_owned()
                })
                .collect();
            CapNodeDto {
                name,
                functions: fns,
            }
        })
        .collect();

    SymbolGraphDto {
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
