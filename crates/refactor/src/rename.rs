//! Rename symbol refactor.
//!
//! CST-based: walks the rowan syntax tree to find all `Ident` tokens
//! matching the old name, filters by syntactic context (parent node kind)
//! to determine if the token refers to the target symbol kind, and
//! produces text edits.

use kyokara_hir::{CheckResult, ProjectCheckResult};
use kyokara_intern::Interner;
use kyokara_parser::SyntaxKind;
use kyokara_span::FileId;
use kyokara_syntax::SyntaxNode;

use crate::{RefactorError, RefactorResult, SymbolKind, TextEdit};

/// Rename a symbol in a single-file check result.
pub fn rename_symbol(
    result: &CheckResult,
    file_id: FileId,
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
) -> Result<RefactorResult, RefactorError> {
    // 1. Validate the old name exists in the module scope.
    validate_old_name_exists(&result.interner, &result.module_scope, old_name, kind)?;

    // 2. Check the new name doesn't conflict.
    validate_new_name(&result.interner, &result.module_scope, new_name, kind)?;

    // 3. Check the new name is not a keyword.
    if SyntaxKind::from_keyword(new_name).is_some() {
        return Err(RefactorError::NewNameIsKeyword {
            name: new_name.to_string(),
        });
    }

    // 4. Build CST root and collect edits.
    let root = SyntaxNode::new_root(result.green.clone());
    let edits = collect_rename_edits(&root, file_id, old_name, new_name, kind);

    Ok(RefactorResult {
        description: format!("rename {kind:?} `{old_name}` → `{new_name}`"),
        edits,
        verified: false,
    })
}

/// Rename a symbol across all modules in a multi-file project.
pub fn rename_symbol_project(
    result: &ProjectCheckResult,
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
) -> Result<RefactorResult, RefactorError> {
    // Find the module where the symbol is defined.
    let mut found = false;
    for (_, info) in result.module_graph.iter() {
        if name_exists_in_scope(&result.interner, &info.scope, old_name, kind) {
            found = true;
            break;
        }
    }
    if !found {
        return Err(RefactorError::SymbolNotFound {
            name: old_name.to_string(),
            kind,
        });
    }

    // Check the new name is not a keyword.
    if SyntaxKind::from_keyword(new_name).is_some() {
        return Err(RefactorError::NewNameIsKeyword {
            name: new_name.to_string(),
        });
    }

    // Check for conflicts in every module.
    for (_, info) in result.module_graph.iter() {
        if let Err(e) = validate_new_name(&result.interner, &info.scope, new_name, kind) {
            return Err(e);
        }
    }

    // Walk each module's source and collect edits.
    let mut all_edits = Vec::new();
    for (_, info) in result.module_graph.iter() {
        let parse = kyokara_syntax::parse(&info.source);
        let root = SyntaxNode::new_root(parse.green);
        let edits = collect_rename_edits(&root, info.file_id, old_name, new_name, kind);
        all_edits.extend(edits);
    }

    Ok(RefactorResult {
        description: format!("rename {kind:?} `{old_name}` → `{new_name}` (project-wide)"),
        edits: all_edits,
        verified: false,
    })
}

// ── Validation helpers ──────────────────────────────────────────────

fn validate_old_name_exists(
    interner: &Interner,
    scope: &kyokara_hir::ModuleScope,
    old_name: &str,
    kind: SymbolKind,
) -> Result<(), RefactorError> {
    if !name_exists_in_scope(interner, scope, old_name, kind) {
        return Err(RefactorError::SymbolNotFound {
            name: old_name.to_string(),
            kind,
        });
    }
    Ok(())
}

fn name_exists_in_scope(
    interner: &Interner,
    scope: &kyokara_hir::ModuleScope,
    name: &str,
    kind: SymbolKind,
) -> bool {
    match kind {
        SymbolKind::Function => scope.functions.keys().any(|n| n.resolve(interner) == name),
        SymbolKind::Type => scope.types.keys().any(|n| n.resolve(interner) == name),
        SymbolKind::Capability => scope.caps.keys().any(|n| n.resolve(interner) == name),
        SymbolKind::Variant => scope
            .constructors
            .keys()
            .any(|n| n.resolve(interner) == name),
    }
}

fn validate_new_name(
    interner: &Interner,
    scope: &kyokara_hir::ModuleScope,
    new_name: &str,
    _kind: SymbolKind,
) -> Result<(), RefactorError> {
    // Check if the new name conflicts with any existing symbol.
    if scope
        .functions
        .keys()
        .any(|n| n.resolve(interner) == new_name)
    {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Function,
        });
    }
    if scope.types.keys().any(|n| n.resolve(interner) == new_name) {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Type,
        });
    }
    if scope.caps.keys().any(|n| n.resolve(interner) == new_name) {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Capability,
        });
    }
    if scope
        .constructors
        .keys()
        .any(|n| n.resolve(interner) == new_name)
    {
        return Err(RefactorError::NameConflict {
            name: new_name.to_string(),
            existing_kind: SymbolKind::Variant,
        });
    }

    Ok(())
}

// ── CST walking ─────────────────────────────────────────────────────

/// Collect text edits for renaming `old_name` → `new_name` in a single CST.
fn collect_rename_edits(
    root: &SyntaxNode,
    file_id: FileId,
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::Ident || token.text() != old_name {
            continue;
        }

        let Some(parent) = token.parent() else {
            continue;
        };

        if should_rename_token(&parent, kind) {
            edits.push(TextEdit {
                file_id,
                range: token.text_range(),
                new_text: new_name.to_string(),
            });
        }
    }

    // Sort descending by start offset for correct application order.
    edits.sort_by(|a, b| b.range.start().cmp(&a.range.start()));
    edits
}

/// Determine if an ident token with the given parent should be renamed
/// for the target symbol kind.
pub fn should_rename_token(parent: &SyntaxNode, kind: SymbolKind) -> bool {
    let parent_kind = parent.kind();

    // Definition sites: ident is a direct child of a definition node.
    match kind {
        SymbolKind::Function if parent_kind == SyntaxKind::FnDef => return true,
        SymbolKind::Type if parent_kind == SyntaxKind::TypeDef => return true,
        SymbolKind::Capability if parent_kind == SyntaxKind::CapDef => return true,
        SymbolKind::Variant if parent_kind == SyntaxKind::Variant => return true,
        _ => {}
    }

    // Usage sites: ident is inside a Path node. Check the Path's parent.
    if parent_kind == SyntaxKind::Path {
        let Some(grandparent) = parent.parent() else {
            return false;
        };
        return is_usage_site(grandparent.kind(), kind, &grandparent);
    }

    false
}

/// Check if a Path's parent node kind is a valid usage site for the
/// target symbol kind.
pub fn is_usage_site(gp_kind: SyntaxKind, kind: SymbolKind, grandparent: &SyntaxNode) -> bool {
    match kind {
        SymbolKind::Function => matches!(gp_kind, SyntaxKind::PathExpr | SyntaxKind::CallExpr),
        SymbolKind::Type => matches!(
            gp_kind,
            SyntaxKind::NameType | SyntaxKind::RecordExpr | SyntaxKind::RecordPat
        ),
        SymbolKind::Capability => {
            // Capabilities appear as NameType inside WithClause or PipeClause.
            if gp_kind == SyntaxKind::NameType {
                if let Some(ggp) = grandparent.parent() {
                    return matches!(ggp.kind(), SyntaxKind::WithClause | SyntaxKind::PipeClause);
                }
            }
            // Also match CapDef name position (handled above as definition site).
            false
        }
        SymbolKind::Variant => {
            if matches!(gp_kind, SyntaxKind::ConstructorPat | SyntaxKind::PathExpr) {
                return true;
            }
            // Zero-arg variant in match pattern: IdentPat > Path > Ident
            // But skip if the IdentPat is inside a LetBinding (local variable).
            if gp_kind == SyntaxKind::IdentPat {
                if let Some(ggp) = grandparent.parent() {
                    return ggp.kind() != SyntaxKind::LetBinding;
                }
                return true;
            }
            false
        }
    }
}
